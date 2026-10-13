//! Linux clipboard backend with runtime Wayland / X11 dispatch.
//!
//! `LinuxClipboard` is the public type plugged in as `LocalClipboard` for
//! `target_os = "linux"`. At construction time it probes the session and
//! selects a backend:
//!
//! - `wayland::WaylandClipboard` — native `ext-data-control` /
//!   `wlr-data-control` client (serves both read and write) when the
//!   compositor advertises the data-control manager.
//! - [`LinuxClipboard::WaylandDataDevice`] — a Wayland session with NO
//!   data-control manager (e.g. GNOME mutter): writes go to the native
//!   `wl_data_device` ([`wayland::DataDeviceWriter`]) so GTK/Nautilus (which
//!   read the Wayland selection) can paste, while reads keep using the X11
//!   backend — background Wayland *read/observe* needs data-control or a GNOME
//!   Shell extension and is handled separately (deferred).
//! - [`x11::X11Clipboard`] — native `x11rb` ICCCM selection client for X11
//!   sessions and the final fallback.
//!
//! [`build_event_loop`] follows the same precedence (Wayland → X11) for
//! the change watcher. Each layer's `try_new` returns `Ok(None)` when its
//! probe says "not available here", so the cascade is transparent.
//!
//! Phase 4 removed the legacy `clipboard_rs` fallback — every Linux
//! environment we support is covered by one of the native backends.

pub(super) mod wayland;
pub(super) mod x11;

/// Shared Linux clipboard READ-path MIME mapping, used by BOTH the Wayland
/// (`wayland::snapshot`) and X11 (`x11::reader`) backends. Default-private to
/// the `linux` subtree; the `pub(super)` (== `pub(in ...::linux)`) helpers
/// inside it are reachable from every descendant of `linux`.
mod mime;
mod write_offers;

use crate::clipboard::NoopSystemClipboard;
use crate::clipboard::SystemClipboard;
use crate::clipboard::SystemClipboardSnapshot;
use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};

use crate::clipboard::event_loop::PlatformClipboardEventLoop;

pub enum LinuxClipboard {
    /// Compositor advertises `ext-data-control` or `wlr-data-control`: one
    /// native backend serving both read and write.
    Wayland(wayland::WaylandClipboard),
    /// X11 session (or a Wayland session with no native writer available): the
    /// `x11rb` ICCCM backend serves both read and write.
    X11(x11::X11Clipboard),
    /// Wayland session whose compositor advertises no data-control manager.
    /// **Dual write**: every write goes to the X11 clipboard (ICCCM ownership
    /// needs no focus, so it always works) *and*, best-effort, to the native
    /// Wayland data device so GTK/Nautilus (which read the Wayland selection)
    /// can paste. Reads keep using the X11 backend when an X display is
    /// reachable (or a no-op backend when there is no X display at all).
    ///
    /// Caveat: on GNOME/Mutter the native Wayland write succeeds only when this
    /// process is the keyboard-focus client; a headless daemon is not, so on
    /// GNOME the X11 half is the effective write until the GUI performs the
    /// native write (see `VISION` / the deferred GUI-side write work). The
    /// native write is still emitted so it works on compositors that do not
    /// gate the selection on focus (wlroots).
    ///
    /// Background Wayland *read/observe* needs data-control or a GNOME Shell
    /// extension and is handled separately (deferred).
    WaylandDataDevice {
        wl_writer: wayland::DataDeviceWriter,
        x11: Box<dyn SystemClipboard>,
    },
}

impl LinuxClipboard {
    pub fn new() -> Result<Self> {
        // Preference order:
        //   1. Wayland data-control (ext or wlr) — one native backend serving
        //      both read and write; only when the compositor advertises the
        //      data-control manager.
        //   2. Wayland data-device write + X11 read — Wayland compositors with
        //      no data-control manager (e.g. GNOME): writes go natively to the
        //      Wayland selection so GTK/Nautilus can paste, reads keep X11.
        //   3. Native x11rb — X11 sessions, or any fallback when the above are
        //      unavailable.
        if is_wayland_session() {
            match wayland::WaylandClipboard::try_new() {
                Ok(Some(wl)) => {
                    info!("Linux clipboard: native Wayland (data-control)");
                    return Ok(Self::Wayland(wl));
                }
                Ok(None) => {
                    info!("Linux clipboard: Wayland session but no data-control protocol");
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Linux clipboard: Wayland data-control probe failed"
                    );
                }
            }

            match wayland::DataDeviceWriter::try_new() {
                Ok(Some(wl_writer)) => {
                    let x11: Box<dyn SystemClipboard> = match x11::try_new_clipboard() {
                        Ok(Some(x)) => {
                            info!(
                                "Linux clipboard: dual write (native Wayland data-device + X11) \
                                 with X11 read (no data-control)"
                            );
                            Box::new(x)
                        }
                        Ok(None) => {
                            info!(
                                "Linux clipboard: native Wayland data-device write only (no \
                                 data-control and no X display; reads unavailable)"
                            );
                            Box::new(NoopSystemClipboard)
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
                                "Linux clipboard: X11 backend init failed; native Wayland write \
                                 only"
                            );
                            Box::new(NoopSystemClipboard)
                        }
                    };
                    return Ok(Self::WaylandDataDevice { wl_writer, x11 });
                }
                Ok(None) => {
                    info!(
                        "Linux clipboard: no Wayland data-device writer (missing seat/manager); \
                         falling through to native x11rb"
                    );
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Linux clipboard: Wayland data-device writer init failed; falling \
                         through to native x11rb"
                    );
                }
            }
        }

        match x11::try_new_clipboard() {
            Ok(Some(x)) => {
                info!(
                    wayland_session = is_wayland_session(),
                    "Linux clipboard: native X11 (x11rb)"
                );
                Ok(Self::X11(x))
            }
            Ok(None) => Err(anyhow::anyhow!(
                "Linux clipboard: no usable backend — Wayland data-control unavailable and \
                 no X display reachable (DISPLAY={:?}, WAYLAND_DISPLAY={:?})",
                std::env::var_os("DISPLAY"),
                std::env::var_os("WAYLAND_DISPLAY")
            )),
            Err(e) => Err(e.context("Linux clipboard: native X11 init failed")),
        }
    }
}

#[async_trait]
impl SystemClipboard for LinuxClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        match self {
            Self::Wayland(c) => c.read_snapshot(),
            Self::X11(c) => c.read_snapshot(),
            Self::WaylandDataDevice { x11, .. } => x11.read_snapshot(),
        }
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        match self {
            Self::Wayland(c) => c.write_snapshot(snapshot),
            Self::X11(c) => c.write_snapshot(snapshot),
            Self::WaylandDataDevice { wl_writer, x11 } => {
                // X11 first: ICCCM ownership does not require focus, so it
                // succeeds even when the compositor gates the native Wayland
                // write on the keyboard-focus client (GNOME/Mutter). A failed
                // X11 write must not abort the native write.
                if let Err(e) = x11.write_snapshot(snapshot.clone()) {
                    warn!(
                        error = %e,
                        "Linux clipboard: X11 write failed; continuing to native Wayland write"
                    );
                }
                // Native Wayland: best-effort. Succeeds when this process owns
                // keyboard focus or the compositor does not gate the selection
                // on focus (wlroots); on GNOME a headless daemon is cancelled.
                wl_writer.write_snapshot(snapshot)
            }
        }
    }
}

/// Returns true if the current process is running under a Wayland session.
///
/// Strict check: the `WAYLAND_DISPLAY` environment variable must point at a
/// reachable wayland socket. We do not look at `XDG_SESSION_TYPE` because
/// that's a session manager hint, not a runtime guarantee — XWayland-only
/// processes can have `XDG_SESSION_TYPE=wayland` but no `WAYLAND_DISPLAY`.
pub(super) fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Build the platform clipboard event loop for Linux.
///
/// Runtime selection mirrors [`LinuxClipboard::new`]:
///
/// 1. Wayland data-control via [`wayland::WaylandEventLoop`] if the
///    compositor advertises ext- or wlr-data-control.
/// 2. Native x11rb via [`x11::X11EventLoop`] whenever an X display is
///    reachable (covers XWayland sessions whose compositor doesn't expose
///    data-control).
///
/// Returns `Err` if neither backend can be brought up — that means the
/// process is on a Linux box with neither a Wayland nor an X display, which
/// is unusual for desktop deployments (headless containers should not be
/// running the daemon).
pub(super) fn build_event_loop() -> Result<Box<dyn PlatformClipboardEventLoop>> {
    if is_wayland_session() {
        match wayland::WaylandEventLoop::try_new() {
            Ok(Some(wl)) => {
                info!("Linux clipboard event loop: native Wayland (data-control)");
                return Ok(Box::new(wl));
            }
            Ok(None) => {
                info!(
                    "Linux clipboard event loop: Wayland session but no data-control \
                     protocol; falling through to native x11rb"
                );
            }
            Err(e) => {
                warn!(error = %e, "Linux clipboard event loop: Wayland probe failed; falling through");
            }
        }
    }

    match x11::try_new_event_loop() {
        Ok(Some(loop_)) => {
            info!(
                wayland_session = is_wayland_session(),
                "Linux clipboard event loop: native X11 (x11rb + XFIXES)"
            );
            Ok(Box::new(loop_))
        }
        Ok(None) => Err(anyhow::anyhow!(
            "Linux clipboard event loop: no usable backend — Wayland data-control \
             unavailable and no X display reachable (DISPLAY={:?}, WAYLAND_DISPLAY={:?})",
            std::env::var_os("DISPLAY"),
            std::env::var_os("WAYLAND_DISPLAY")
        )),
        Err(e) => Err(e.context("Linux clipboard event loop: native X11 probe failed")),
    }
}
