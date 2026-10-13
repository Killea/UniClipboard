//! Native Wayland clipboard WRITE backend over the core `wl_data_device`
//! protocol.
//!
//! The data-control protocols in [`super::protocol`] (`ext-data-control-v1` /
//! `wlr-data-control-unstable-v1`) let a *background* client READ the clipboard
//! without keyboard focus — exactly what a headless daemon needs to observe
//! changes. GNOME mutter advertises NEITHER manager, so those backends are
//! unavailable there. Writing, however, does not need data-control: the core
//! `wl_data_device` protocol (`set_selection` + `wl_data_source`) is what
//! `wl-copy` / GTK / Nautilus use, and it is advertised by every Wayland
//! compositor, GNOME included.
//!
//! This module implements only the WRITE half on that core protocol: install a
//! `wl_data_source` carrying the offered mimes via `set_selection`, then serve
//! each `Send { mime_type, fd }` event by writing the payload into the
//! compositor-provided fd (the same mechanism the data-control backends use,
//! see [`super::write_payload`]). It is deliberately write-only: reading the
//! clipboard in the background still needs data-control or a GNOME Shell
//! extension, so [`super::super::LinuxClipboard`] keeps an X11 read backend
//! alongside this writer when data-control is absent.
//!
//! ## Serial
//!
//! `set_selection(source, serial)` wants the keyboard-grab serial that triggered
//! the copy. A background daemon has no focused keyboard, so this defaults to
//! serial 0 (which GNOME/mutter accepts for the clipboard selection) and honours
//! `UC_WAYLAND_SELECTION_SERIAL` for development. If a target compositor rejects
//! serial 0, the GUI — which does hold the grab serial — must supply one.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::sync::mpsc::{self, sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use wayland_client::protocol::{
    wl_data_device, wl_data_device::WlDataDevice, wl_data_device_manager,
    wl_data_device_manager::WlDataDeviceManager, wl_data_offer, wl_data_offer::WlDataOffer,
    wl_data_source, wl_data_source::WlDataSource, wl_registry, wl_registry::WlRegistry, wl_seat,
    wl_seat::WlSeat,
};
use wayland_client::{event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use crate::clipboard::SystemClipboardSnapshot;

use super::offer_plan::offer_plan;
use super::write_payload::write_payload;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const WL_SEAT_VERSION: u32 = 1;
const WL_DATA_DEVICE_MANAGER_VERSION: u32 = 3;

/// `set_selection` serial override for development (see module docs).
const SERIAL_ENV: &str = "UC_WAYLAND_SELECTION_SERIAL";

fn selection_serial() -> u32 {
    std::env::var(SERIAL_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Writer facade
// ---------------------------------------------------------------------------

pub struct DataDeviceWriter {
    inner: Arc<Inner>,
}

struct Inner {
    request_tx: mpsc::Sender<Request>,
    wakeup_fd: OwnedFd,
    worker: std::sync::Mutex<Option<JoinHandle<()>>>,
}

enum Request {
    Write(SystemClipboardSnapshot, SyncSender<Result<()>>),
    Stop,
}

impl DataDeviceWriter {
    /// Connect to the running Wayland session and bring up the native write
    /// worker.
    ///
    /// - `Ok(Some(_))` — a `wl_seat` + `wl_data_device_manager` are advertised
    ///   and the worker is running; `write_snapshot` installs selections.
    /// - `Ok(None)` — connected but a core global is missing (no seat, e.g. a
    ///   headless remote session); caller falls back to X11.
    /// - `Err(_)` — connect / spawn failure.
    pub(crate) fn try_new() -> Result<Option<Self>> {
        let conn = match Connection::connect_to_env() {
            Ok(c) => c,
            Err(e) => {
                debug!(error = %e, "wayland data-device writer: cannot connect; skipping");
                return Ok(None);
            }
        };

        // Probe the core globals we need. `get_data_device` requires a seat, so a
        // session with no seat cannot install a selection and we defer to X11.
        let mut probe = conn.new_event_queue::<ProbeState>();
        let probe_qh = probe.handle();
        let _registry = conn.display().get_registry(&probe_qh, ());
        let mut probe_state = ProbeState::default();
        probe
            .roundtrip(&mut probe_state)
            .context("wayland data-device writer probe roundtrip failed")?;
        if !probe_state.has_seat || !probe_state.has_manager {
            debug!(
                has_seat = probe_state.has_seat,
                has_manager = probe_state.has_manager,
                "wayland data-device writer: missing core global; skipping"
            );
            return Ok(None);
        }

        let wakeup_fd = rustix::event::eventfd(
            0,
            rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
        )
        .context("creating wayland data-device wakeup eventfd")?;

        let worker_wakeup_fd = wakeup_fd
            .try_clone()
            .context("dup wayland data-device wakeup eventfd for worker")?;

        let (request_tx, request_rx) = mpsc::channel::<Request>();

        let worker = std::thread::Builder::new()
            .name("wayland-data-device-writer".into())
            .spawn(move || {
                if let Err(e) = worker_main(conn, request_rx, worker_wakeup_fd) {
                    warn!(error = ?e, "wayland data-device writer worker exited with error");
                }
            })
            .context("spawning wayland data-device writer worker thread")?;

        info!("wayland data-device writer: started (native Wayland clipboard write path)");
        Ok(Some(Self {
            inner: Arc::new(Inner {
                request_tx,
                wakeup_fd,
                worker: std::sync::Mutex::new(Some(worker)),
            }),
        }))
    }

    /// Install `snapshot` on the native Wayland selection. Returns when the
    /// selection is installed (or the worker reports an error / times out).
    pub(crate) fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        let (tx, rx) = sync_channel::<Result<()>>(1);
        self.send_request(Request::Write(snapshot, tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "wayland data-device write timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }

    fn send_request(&self, req: Request) -> Result<()> {
        self.inner
            .request_tx
            .send(req)
            .map_err(|e| anyhow::anyhow!("wayland data-device worker channel closed: {e}"))?;
        let buf = 1u64.to_ne_bytes();
        if let Err(e) = rustix::io::write(&self.inner.wakeup_fd, &buf) {
            warn!(error = %e, "wayland data-device wakeup write failed");
        }
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.request_tx.send(Request::Stop);
        let buf = 1u64.to_ne_bytes();
        let _ = rustix::io::write(&self.wakeup_fd, &buf);
        if let Some(handle) = self.worker.lock().ok().and_then(|mut g| g.take()) {
            if let Err(e) = handle.join() {
                warn!(?e, "wayland data-device writer worker panicked on join");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ProbeState {
    has_seat: bool,
    has_manager: bool,
}

impl Dispatch<WlRegistry, ()> for ProbeState {
    fn event(
        s: &mut Self,
        _r: &WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = e {
            match interface.as_str() {
                "wl_seat" => s.has_seat = true,
                "wl_data_device_manager" => s.has_manager = true,
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

struct ActiveSource {
    source: WlDataSource,
    payloads: HashMap<String, Arc<Vec<u8>>>,
}

struct WorkerState {
    seat: Option<WlSeat>,
    manager: Option<WlDataDeviceManager>,
    device: Option<WlDataDevice>,
    active_source: Option<ActiveSource>,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            seat: None,
            manager: None,
            device: None,
            active_source: None,
        }
    }
}

fn worker_main(conn: Connection, request_rx: Receiver<Request>, wakeup_fd: OwnedFd) -> Result<()> {
    info!("wayland data-device writer worker: starting");

    let mut event_queue: EventQueue<WorkerState> = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut state = WorkerState::new();

    event_queue
        .roundtrip(&mut state)
        .context("wayland data-device writer startup roundtrip failed")?;

    let manager = state
        .manager
        .clone()
        .context("wl_data_device_manager disappeared after probe")?;
    let seat = state
        .seat
        .clone()
        .context("wl_seat disappeared after probe")?;

    state.device = Some(manager.get_data_device(&seat, &qh, ()));

    event_queue
        .roundtrip(&mut state)
        .context("wayland data-device writer device-bind roundtrip failed")?;

    loop {
        event_queue
            .dispatch_pending(&mut state)
            .context("wayland data-device writer dispatch_pending failed")?;

        loop {
            match request_rx.try_recv() {
                Ok(Request::Write(snap, reply)) => {
                    let res = handle_write(&mut state, &qh, snap);
                    let _ = reply.send(res);
                }
                Ok(Request::Stop) => {
                    info!("wayland data-device writer worker: stop request received");
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    info!("wayland data-device writer worker: request channel disconnected");
                    return Ok(());
                }
            }
        }

        event_queue
            .flush()
            .context("wayland data-device writer event_queue flush failed")?;

        let read_guard = match conn.prepare_read() {
            Some(g) => g,
            None => continue,
        };
        let wl_raw_fd = read_guard.connection_fd().as_raw_fd();
        let wakeup_raw_fd = wakeup_fd.as_raw_fd();

        // SAFETY: both fds outlive this poll call (`read_guard` keeps the
        // wayland fd alive, `wakeup_fd` is owned by the worker stack).
        let wl_borrow = unsafe { BorrowedFd::borrow_raw(wl_raw_fd) };
        let wakeup_borrow = unsafe { BorrowedFd::borrow_raw(wakeup_raw_fd) };
        let mut pfds = [
            PollFd::new(&wl_borrow, PollFlags::IN),
            PollFd::new(&wakeup_borrow, PollFlags::IN),
        ];

        match poll(&mut pfds, -1) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => {
                drop(read_guard);
                continue;
            }
            Err(e) => return Err(e.into()),
        }

        let wl_revents = pfds[0].revents();
        let wakeup_revents = pfds[1].revents();

        if wakeup_revents.contains(PollFlags::IN) {
            let mut buf = [0u8; 8];
            let _ = rustix::io::read(&wakeup_fd, &mut buf);
        }

        if wl_revents.contains(PollFlags::IN) {
            if let Err(e) = read_guard.read() {
                return Err(anyhow::anyhow!(
                    "wayland data-device writer read events failed: {e:?}"
                ));
            }
        } else {
            drop(read_guard);
        }
    }
}

fn handle_write(
    state: &mut WorkerState,
    qh: &QueueHandle<WorkerState>,
    snapshot: SystemClipboardSnapshot,
) -> Result<()> {
    let device = state
        .device
        .as_ref()
        .context("wayland data-device writer has no data device")?
        .clone();

    if snapshot.representations.is_empty() {
        // Clear the selection. Do NOT destroy the previous source eagerly:
        // `set_selection(None)` makes the compositor cancel it and the
        // `Cancelled` handler destroys it (mirrors the data-control writers).
        state.active_source = None;
        device.set_selection(None, 0);
        return Ok(());
    }

    let source = state
        .manager
        .as_ref()
        .context("wayland data-device writer has no data device manager")?
        .create_data_source(qh, ());

    let offers = offer_plan(&snapshot);
    let payloads: HashMap<String, Arc<Vec<u8>>> = offers
        .iter()
        .map(|(mime, bytes)| (mime.clone(), Arc::clone(bytes)))
        .collect();

    if payloads.is_empty() {
        source.destroy();
        anyhow::bail!("wayland data-device write: no mime could be derived from snapshot");
    }

    for (mime, _) in &offers {
        source.offer(mime.clone());
    }

    let serial = selection_serial();
    info!(
        serial,
        mime_count = offers.len(),
        "wayland data-device writer: set_selection"
    );
    // `set_selection` atomically replaces the selection; the compositor sends
    // `Cancelled` for the previous source, which the source handler destroys.
    device.set_selection(Some(&source), serial);
    state.active_source = Some(ActiveSource { source, payloads });
    Ok(())
}

// ---------------------------------------------------------------------------
// Worker dispatch impls
// ---------------------------------------------------------------------------

impl Dispatch<WlRegistry, ()> for WorkerState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    let v = version.min(WL_SEAT_VERSION);
                    state.seat = Some(registry.bind::<WlSeat, (), Self>(name, v, qh, ()));
                }
                "wl_data_device_manager" if state.manager.is_none() => {
                    let v = version.min(WL_DATA_DEVICE_MANAGER_VERSION);
                    state.manager =
                        Some(registry.bind::<WlDataDeviceManager, (), Self>(name, v, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDeviceManager, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: wl_data_device_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// Write-only: this device never reads the selection, so the `data_offer`
// objects it receives (including self-echoes of our own `set_selection`) are
// destroyed immediately. The `event_created_child!` registration is mandatory —
// without it the default handler panics the moment the compositor delivers a
// `data_offer` new-id.
impl Dispatch<WlDataDevice, ()> for WorkerState {
    fn event(
        state: &mut Self,
        _device: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => {
                // Destroy now: we serve no offers, so a live proxy would only
                // accumulate (one per clipboard change) without ever being read.
                // `state` is unused beyond this — the write-only writer never
                // inspects offer contents.
                let _ = state;
                id.destroy();
            }
            _ => {}
        }
    }

    event_created_child!(WorkerState, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

// Offers are destroyed on arrival in the data-device handler. The impl must
// still exist because `event_created_child!` registers this proxy type; it
// normally receives no events (the proxy is already destroyed by the time its
// mime `offer` events would arrive).
impl Dispatch<WlDataOffer, ()> for WorkerState {
    fn event(
        _: &mut Self,
        offer: &WlDataOffer,
        _: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        offer.destroy();
    }
}

impl Dispatch<WlDataSource, ()> for WorkerState {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                let active = match &state.active_source {
                    Some(a) if a.source.id() == source.id() => a,
                    _ => {
                        debug!(
                            mime = %mime_type,
                            "wayland data-device writer: Send for stale source — ignoring"
                        );
                        return;
                    }
                };
                match active.payloads.get(&mime_type) {
                    Some(bytes) => {
                        write_payload(fd, bytes, &mime_type);
                    }
                    None => {
                        debug!(
                            mime = %mime_type,
                            "wayland data-device writer: paster requested mime we don't carry — closing fd"
                        );
                        drop(fd);
                    }
                }
            }
            wl_data_source::Event::Cancelled => {
                // A source is cancelled either because another client took over
                // the selection, or because we replaced our own source via a fresh
                // `set_selection`. Destroy it in both cases to release the proxy.
                let is_active = state
                    .active_source
                    .as_ref()
                    .map(|active| active.source.id() == source.id())
                    .unwrap_or(false);
                if is_active {
                    debug!("wayland data-device writer: active source cancelled by compositor");
                    state.active_source = None;
                } else {
                    debug!("wayland data-device writer: replaced source cancelled — cleaning up");
                }
                source.destroy();
            }
            _ => {}
        }
    }
}
