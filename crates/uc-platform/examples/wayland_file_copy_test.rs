//! Smoke test for the native Wayland data-device WRITE path with a FILE set.
//!
//! Verifies that a file-set snapshot reaches the compositor's clipboard with
//! the MIME types Nautilus/GTK require to enable file Paste:
//! `text/uri-list`, `x-special/gnome-copied-files`, and a `text/plain` fallback.
//! This is the regression that motivated the data-device writer: on GNOME
//! (no data-control), writes must go to the native Wayland selection so a
//! file-manager Paste works.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG="info,uc_platform=debug" cargo run --example wayland_file_copy_test -p uc-platform
//! ```
//!
//! The process owns the selection it installs; run it in the foreground and it
//! serves `wl-paste` / Nautilus while alive.

use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use uc_platform::clipboard::{
    LocalClipboard, MimeType, ObservedClipboardRepresentation, RepresentationId, SystemClipboard,
    SystemClipboardSnapshot,
};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,uc_platform=debug")),
        )
        .with_target(true)
        .init();

    eprintln!(
        "(WAYLAND_DISPLAY={:?})",
        std::env::var_os("WAYLAND_DISPLAY")
    );

    let clipboard: Arc<dyn SystemClipboard> = Arc::new(LocalClipboard::new()?);

    // Build a real file:// URI list for three temp files.
    let dir = std::env::temp_dir().join("uc-file-copy-test");
    std::fs::create_dir_all(&dir)?;
    let uris: Vec<String> = ["alpha.txt", "beta.txt", "gamma.txt"]
        .iter()
        .map(|n| {
            let path = dir.join(n);
            std::fs::write(&path, b"hello file-copy").unwrap();
            format!("file://{}", path.display())
        })
        .collect();
    // RFC 2483: newline-terminated URI list.
    let uri_list: String = uris.join("\n") + "\n";
    eprintln!("file set URI list:\n{uri_list}");

    let snap = SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            "files".into(),
            Some(MimeType("text/uri-list".into())),
            uri_list.clone().into_bytes(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    eprintln!("[1] writing file set via the native Wayland data device...");
    clipboard.write_snapshot(snap)?;
    eprintln!("    write OK; letting the compositor settle...");
    std::thread::sleep(Duration::from_millis(500));

    eprintln!("[2] wl-paste --list-types:");
    let out = Command::new("wl-paste").arg("--list-types").output()?;
    eprintln!("{}", String::from_utf8_lossy(&out.stdout));

    eprintln!("[3] wl-paste --type text/uri-list:");
    let out = Command::new("wl-paste")
        .arg("--type")
        .arg("text/uri-list")
        .output()?;
    eprintln!("{}", String::from_utf8_lossy(&out.stdout));

    eprintln!("[4] wl-paste --type x-special/gnome-copied-files:");
    let out = Command::new("wl-paste")
        .arg("--type")
        .arg("x-special/gnome-copied-files")
        .output()?;
    eprintln!("{}", String::from_utf8_lossy(&out.stdout));

    eprintln!("[5] wl-paste --type text/plain:");
    let out = Command::new("wl-paste")
        .arg("--type")
        .arg("text/plain")
        .output()?;
    eprintln!("{}", String::from_utf8_lossy(&out.stdout));

    eprintln!("\ndone — this process now owns the selection; paste into Nautilus (Ctrl+V).");
    eprintln!("Press Ctrl+C to release the selection.");
    // Keep the owner alive so the data can be served to Nautilus / wl-paste.
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
