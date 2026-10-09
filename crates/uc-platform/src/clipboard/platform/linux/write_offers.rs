//! Shared Linux clipboard WRITE-path offers for file-list payloads.
//!
//! Advertising `text/uri-list` alone does not reproduce a native file
//! manager's copy: GNOME Files (Nautilus) and the GTK file-manager family
//! only enable file Paste when the selection carries
//! `x-special/gnome-copied-files` ("copy"|"cut" first line, then URIs), and
//! text targets want a plain-text path fallback (Nautilus itself always
//! advertises `text/plain` alongside the URI list).
//!
//! This module derives those extra offers from the uri-list bytes so a
//! synced file set pastes like a native file copy on every Linux backend
//! (Wayland data-control and X11 ICCCM). Ownership note: the offers are
//! derived inside the platform layer because they are a Linux display-
//! protocol convention, not a business rule — the snapshot model stays
//! untouched.

use url::Url;

/// Atom/mime GTK file managers (Nautilus, Thunar, Caja) require to enable
/// file Paste. Payload: `copy\n<uri>\n<uri>` — LF-separated, with NO trailing
/// newline: Nautilus's `nautilus_clipboard_from_string` rejects any empty
/// line, so a trailing newline would fail the whole parse. We only ever copy,
/// never cut, so the action line is fixed.
pub(super) const GNOME_COPIED_FILES_MIME: &str = "x-special/gnome-copied-files";

/// Is `mime` a file-list payload (`text/uri-list` family) that deserves the
/// extra file-manager offers? Mirrors `MimeType::is_uri_list`.
pub(super) fn is_uri_list_mime(mime: &str) -> bool {
    mime.trim().eq_ignore_ascii_case("text/uri-list")
        || mime.trim().eq_ignore_ascii_case("file/uri-list")
}

/// Given the raw bytes of a `text/uri-list` rep, derive the extra
/// `(mime, bytes)` pairs a native file-manager copy advertises:
///
/// - `x-special/gnome-copied-files` = `"copy\n" + <uris LF-joined>` (no
///   trailing newline — see `GNOME_COPIED_FILES_MIME`) — required by
///   Nautilus/Thunar/Caja; without it Paste stays disabled.
/// - `text/plain` = newline-joined decoded local paths — the fallback text
///   representation file managers also publish, so editors/terminals paste
///   usable paths instead of nothing.
///
/// Callers insert these with LOW priority (`entry().or_insert` /
/// `contains_key` guard) so a genuine `text/plain` rep in the same
/// snapshot always wins over the derived copy.
///
/// Returns `Vec::new()` when the payload carries no usable URIs.
pub(super) fn extra_offers_for_uri_list(uri_list: &[u8]) -> Vec<(String, Vec<u8>)> {
    let text = String::from_utf8_lossy(uri_list);
    // RFC 2483: lines are CRLF-terminated; '#' starts a comment line.
    let uris: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if uris.is_empty() {
        return Vec::new();
    }

    let mut offers = Vec::with_capacity(2);

    offers.push((
        GNOME_COPIED_FILES_MIME.to_string(),
        format!("copy\n{}", uris.join("\n")).into_bytes(),
    ));

    let paths: Vec<String> = uris.iter().map(|uri| uri_to_display_path(uri)).collect();
    offers.push(("text/plain".to_string(), paths.join("\n").into_bytes()));

    offers
}

/// Human-facing text for one URI: local `file://` URIs become the decoded
/// filesystem path (terminals/editors paste usable paths); other schemes
/// keep the raw URI string.
fn uri_to_display_path(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| uri.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_gnome_copied_files_and_text_plain() {
        let uris = b"file:///home/u/a.pdf\r\nfile:///home/u/b%20x.bin\r\n";
        let offers = extra_offers_for_uri_list(uris);
        assert_eq!(offers.len(), 2);

        let gnome = offers
            .iter()
            .find(|(m, _)| m == GNOME_COPIED_FILES_MIME)
            .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
            .unwrap();
        assert_eq!(
            gnome,
            "copy\nfile:///home/u/a.pdf\nfile:///home/u/b%20x.bin"
        );

        let plain = offers
            .iter()
            .find(|(m, _)| m == "text/plain")
            .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
            .unwrap();
        assert_eq!(plain, "/home/u/a.pdf\n/home/u/b x.bin");
    }

    #[test]
    fn normalizes_crlf_and_skips_comments() {
        let uris = b"# comment\r\nfile:///tmp/x\r\n\r\nfile:///tmp/y\n";
        let offers = extra_offers_for_uri_list(uris);
        let gnome = String::from_utf8(offers[0].1.clone()).unwrap();
        assert_eq!(gnome, "copy\nfile:///tmp/x\nfile:///tmp/y");
    }

    /// Nautilus `nautilus_clipboard_from_string` parity: `copy\n`-prefixed
    /// lines split on `\n`, no empty lines allowed, every line a URI. A
    /// trailing newline yields a final empty line → whole parse fails → the
    /// file paste is silently skipped (only e.g. image/png remains usable).
    #[test]
    fn gnome_payload_survives_nautilus_strict_parse() {
        let uris = b"file:///a/x.txt\r\nfile:///a/y.txt\r\n";
        let offers = extra_offers_for_uri_list(uris);
        let gnome = offers
            .iter()
            .find(|(m, _)| m == GNOME_COPIED_FILES_MIME)
            .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
            .unwrap();
        let mut lines = gnome.split('\n');
        assert!(matches!(lines.next(), Some("copy") | Some("cut")));
        for line in lines {
            assert!(!line.is_empty(), "empty line breaks Nautilus parse");
            assert!(Url::parse(line).is_ok(), "invalid URI line: {line}");
        }
    }

    #[test]
    fn keeps_non_file_uris_as_text() {
        let uris = b"https://example.com/x\r\n";
        let offers = extra_offers_for_uri_list(uris);
        let plain = offers
            .iter()
            .find(|(m, _)| m == "text/plain")
            .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
            .unwrap();
        assert_eq!(plain, "https://example.com/x");
    }

    #[test]
    fn empty_payload_yields_no_offers() {
        assert!(extra_offers_for_uri_list(b"").is_empty());
        assert!(extra_offers_for_uri_list(b"# only a comment\n").is_empty());
    }

    #[test]
    fn uri_list_mime_predicate_matches_family() {
        assert!(is_uri_list_mime("text/uri-list"));
        assert!(is_uri_list_mime("file/uri-list"));
        assert!(!is_uri_list_mime("text/plain"));
        assert!(!is_uri_list_mime("image/png"));
    }
}
