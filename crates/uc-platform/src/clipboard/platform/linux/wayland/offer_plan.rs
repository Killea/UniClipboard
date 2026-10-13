//! Shared offer computation for Linux Wayland clipboard WRITE paths.
//!
//! Given a [`SystemClipboardSnapshot`], compute the ordered set of
//! `(mime_type, payload)` offers a Wayland write should advertise. The core
//! data-device backend ([`super::data_device`]) and the data-control backends
//! ([`super::protocol::ext`], [`super::protocol::wlr`]) must all advertise the
//! same MIME family for the same snapshot, so the derivation lives here once
//! instead of being duplicated per protocol.
//!
//! The result is in advertise order: a representation's own MIME aliases first
//! (canonical type leading), then the derived file-manager offers (see
//! [`super::super::write_offers`]). A genuine representation always wins a MIME
//! over a derived copy — the first occurrence of a MIME is kept and later
//! duplicates are dropped.
//!
//! Returns `Vec::new()` when no MIME could be derived from the snapshot; the
//! caller should then destroy the data source and surface an error.

use std::collections::HashSet;
use std::sync::Arc;

use tracing::warn;

use crate::clipboard::payload::rep_bytes;
use crate::clipboard::SystemClipboardSnapshot;

use super::super::write_offers::{extra_offers_for_uri_list, is_uri_list_mime};
use super::protocol::{default_mime_for_format, offer_mimes_for};

/// Compute the ordered `(mime_type, payload)` offers a clipboard write should
/// advertise for `snapshot`. See the module docs for ordering and precedence.
pub(super) fn offer_plan(snapshot: &SystemClipboardSnapshot) -> Vec<(String, Arc<Vec<u8>>)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered: Vec<(String, Arc<Vec<u8>>)> = Vec::new();
    // Derived file-manager offers collected while iterating reps, applied
    // afterwards with a `seen` guard so a genuine rep always wins a mime over a
    // derived copy (e.g. a real `text/plain` beats the derived file paths).
    let mut deferred: Vec<(String, Vec<u8>)> = Vec::new();

    for rep in &snapshot.representations {
        let Some(primary_mime) = rep
            .mime
            .as_ref()
            .map(|m| m.0.clone())
            .or_else(|| default_mime_for_format(&rep.format_id).map(String::from))
        else {
            continue;
        };

        // Use `rep_bytes` (not `expect_inline_bytes`): a synced image rep is a
        // `LocalFile` source pointing at the managed file cache, and reading it
        // can fail on I/O. On failure skip this mime + warn, matching the macOS /
        // Windows writer semantics.
        let bytes = match rep_bytes(rep) {
            Ok(b) => Arc::new(b.into_owned()),
            Err(err) => {
                warn!(
                    error = %err,
                    format_id = %rep.format_id,
                    mime = %primary_mime,
                    "wayland write: read LocalFile rep failed; skipping this mime"
                );
                continue;
            }
        };

        // Advertise every MIME alias a paster might request (text expands to the
        // full UTF-8 family) so apps like Firefox that only negotiate
        // `text/plain;charset=utf-8` / `UTF8_STRING` can paste. All aliases share
        // the same payload bytes.
        for mime in offer_mimes_for(&primary_mime) {
            if seen.contains(&mime) {
                continue;
            }
            seen.insert(mime.clone());
            ordered.push((mime, Arc::clone(&bytes)));
        }

        // File sets additionally advertise the payload variants Linux file
        // managers expect (x-special/gnome-copied-files for Nautilus paste,
        // text/plain paths as the text fallback). See write_offers.rs.
        if is_uri_list_mime(&primary_mime) {
            for (mime, extra) in extra_offers_for_uri_list(&bytes) {
                for offered in offer_mimes_for(&mime) {
                    deferred.push((offered, extra.clone()));
                }
            }
        }
    }

    for (mime, bytes) in deferred {
        if seen.contains(&mime) {
            continue;
        }
        seen.insert(mime.clone());
        ordered.push((mime, Arc::new(bytes)));
    }

    ordered
}
