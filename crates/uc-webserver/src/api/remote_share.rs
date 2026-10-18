//! HTTP handlers for the remote-share (device file share) endpoints.
//!
//! Route table and semantics follow
//! `docs/specs/2026-10-12-device-file-share-spec.md` §9/§12: every handler is
//! a thin mapping onto the session-scoped `uc-engine` remote-share
//! `Operation`s; byte payloads never cross this API (they go over iroh-blobs
//! on the peer side and the daemon's local filesystem on this side).
//!
//! # Session-locked semantics
//!
//! All remote-share ops are session-scoped: when the Space is locked or the
//! session is absent, the engine returns code `1103` (no session) or `2121`
//! (share manifest unreadable while locked). Every route except
//! `GET /remote-share/state` translates that into `423 session_locked`;
//! `GET /remote-share/state` answers the locked shape
//! `200 { locked: true, suspended: null, roots: null }` instead so the
//! settings page can render without exposing paths.
//!
//! # Transfer bookkeeping
//!
//! Upload/download ops take a caller-issued `transfer_id`: the handler mints
//! a UUID, registers it in [`RemoteShareTracker`], spawns the `execute` on a
//! background task (the op itself carries the whole transfer lifecycle), and
//! returns `202 { transferId }` immediately. The daemon's engine-event
//! forwarder uses the tracker to retag `file-transfer.*` events for those ids
//! as `remote-share.*` — `entry_id` alone cannot discriminate (the outbound
//! progress translator copies `transfer_id` into `entry_id` and mobile-lan
//! emits `entry_id: None` events).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::{info_span, warn, Instrument};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::envelope::{
    PeerShareEntryEnvelope, PeerShareListEnvelope, PeerShareMutationResultEnvelope,
    PeerShareStateEnvelope, PeerShareTransferCancelEnvelope, PeerShareTransferStartedEnvelope,
    PeerShareTrashPageEnvelope, RemoteShareReconcileEnvelope, RemoteShareStateEnvelope,
};
use uc_daemon_contract::api::dto::remote_share::{
    PeerShareAvailability, PeerShareCopyRequest, PeerShareDeleteRequest, PeerShareDownloadRequest,
    PeerShareEmptyTrashRequest, PeerShareEntryDto, PeerShareListResponse, PeerShareMkdirRequest,
    PeerShareMutationResultDto, PeerShareRenameRequest, PeerShareRestoreRequest, PeerShareRootDto,
    PeerShareStateDto, PeerShareTransferCancelOutcomeDto, PeerShareTransferCancelResultDto,
    PeerShareTransferStartedDto, PeerShareTrashEntryDto, PeerShareTrashPageDto,
    PeerShareUploadRequest, RemoteShareReconcileDto, RemoteShareRootDto, RemoteShareRootInputDto,
    RemoteShareStateDto, ReplaceRemoteShareRootsRequest, SetRemoteShareSuspendedRequest,
};
use uc_daemon_contract::constants::{
    daemon_error_code, http_route, remote_share_code, ws_event, ws_topic,
};
use uc_engine::error_codes::{
    OPERATION_UNAVAILABLE_CODE, PEER_SHARE_CANCELLED_CODE, PEER_SHARE_CONFLICT_CODE,
    PEER_SHARE_DENIED_CODE, PEER_SHARE_FAILED_CODE, PEER_SHARE_INTERRUPTED_CODE,
    PEER_SHARE_IO_CODE, PEER_SHARE_LOCKED_CODE, PEER_SHARE_NOT_FOUND_CODE, PEER_SHARE_QUOTA_CODE,
    PEER_SHARE_RESERVED_NAME_CODE, PEER_SHARE_REVOKED_CODE, PEER_SHARE_ROOT_GONE_CODE,
    PEER_SHARE_SUSPENDED_CODE, PEER_SHARE_TOO_LARGE_CODE, PEER_SHARE_TOO_LATE_CODE,
    PEER_SHARE_TOO_MANY_ENTRIES_CODE, PEER_SHARE_UNAVAILABLE_CODE, PEER_SHARE_UNSUPPORTED_CODE,
    PEER_SHARE_UNSUPPORTED_REQUEST_CODE, REMOTE_SHARE_LOCKED_CODE, REMOTE_SHARE_STATE_FAILED_CODE,
};
use uc_engine::{
    CancelPeerShareTransferInput, DownloadPeerShareInput, EngineError, EngineErrorCategory,
    MutatePeerShareInput, Operation, OperationResult, PeerShareAction, PeerShareListInput,
    PeerShareRelInput, PeerShareRequest, PeerShareState, PeerShareTrashListInput,
    RemoteShareReconcileSummary, RemoteShareRootInput, RemoteShareRootSummary,
    RemoteShareStateSummary, ReplaceRemoteShareRootsInput, SetRemoteShareSuspendedInput,
    UploadPeerShareInput,
};
use utoipa::{self, IntoParams};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;
use crate::api::types::DaemonWsEvent;

// ============================================================================
// RemoteShareTracker — the daemon-side transfer-id registry
// ============================================================================

/// Classification of a `transfer_id` the daemon sees on the engine event bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteShareTransferMark {
    /// Not a remote-share transfer (clipboard file sync, mobile LAN, …).
    Unknown,
    /// Issued by a remote-share upload/download route and still running.
    Inflight,
    /// A remote-share transfer whose terminal event was already delivered —
    /// late duplicates (the engine emits both a progress-frame terminal and a
    /// client-side terminal for uploads) are dropped silently.
    Finished,
}

/// Daemon-side registry of caller-issued remote-share transfer ids and the
/// last observed per-peer share availability. `Arc`-backed so every
/// `DaemonApiState` clone and the engine-event forwarder share one registry.
///
/// Entries are removed only by daemon restart; the memory cost is a few dozen
/// bytes per transfer ever issued in a session — negligible.
#[derive(Debug, Clone, Default)]
pub struct RemoteShareTracker {
    inner: Arc<RemoteShareTrackerInner>,
}

#[derive(Debug, Default)]
struct RemoteShareTrackerInner {
    inflight: Mutex<HashSet<String>>,
    finished: Mutex<HashSet<String>>,
    peer_states: Mutex<HashMap<String, PeerShareAvailability>>,
}

impl RemoteShareTracker {
    /// Record a transfer id that a remote-share route just issued.
    pub fn begin(&self, transfer_id: &str) {
        self.inner
            .inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(transfer_id.to_string());
    }

    /// Classify an event's `transfer_id` for WS retagging.
    pub fn mark(&self, transfer_id: &str) -> RemoteShareTransferMark {
        let inflight = self
            .inner
            .inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if inflight.contains(transfer_id) {
            return RemoteShareTransferMark::Inflight;
        }
        drop(inflight);
        if self
            .inner
            .finished
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(transfer_id)
        {
            RemoteShareTransferMark::Finished
        } else {
            RemoteShareTransferMark::Unknown
        }
    }

    /// Move an inflight transfer to finished. Returns `true` iff the id was
    /// still inflight — callers emit the terminal event only then, so duplicate
    /// engine terminals collapse to one WS event.
    pub fn finish(&self, transfer_id: &str) -> bool {
        let mut inflight = self
            .inner
            .inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !inflight.remove(transfer_id) {
            return false;
        }
        drop(inflight);
        self.inner
            .finished
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(transfer_id.to_string());
        true
    }

    /// Observe a peer's share availability; returns `true` on a transition the
    /// WS layer should publish (a first observation is a baseline, not a change).
    pub fn observe_peer_state(&self, peer_id: &str, state: PeerShareAvailability) -> bool {
        let mut states = self
            .inner
            .peer_states
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match states.get(peer_id) {
            Some(previous) if *previous == state => false,
            _ => states.insert(peer_id.to_string(), state).is_some(),
        }
    }
}

// ============================================================================
// Error mapping — engine numeric codes → spec §12 API codes
// ============================================================================

/// `423 session_locked`: every remote-share op is session-scoped.
fn session_locked() -> ApiError {
    ApiError {
        status: StatusCode::LOCKED,
        code: daemon_error_code::SESSION_LOCKED.to_string(),
        message: "encryption session is locked".to_string(),
        details: None,
    }
}

/// True for the two engine codes that mean "this box has no usable session":
/// missing session (`1103`) and share manifest unreadable while locked (`2121`).
fn is_session_locked(error: &EngineError) -> bool {
    matches!(
        error.code(),
        OPERATION_UNAVAILABLE_CODE | REMOTE_SHARE_LOCKED_CODE
    )
}

/// Map an engine error onto the spec §12 API surface. The deny block
/// 2101–2117 maps one-to-one onto `remote_share_code` strings; transport and
/// local faults follow, and anything unexpected falls back to `category`.
fn share_error_to_api(op: &'static str, error: EngineError) -> ApiError {
    use remote_share_code as code;
    if is_session_locked(&error) {
        return session_locked();
    }
    let (variant, api): (&'static str, ApiError) = match error.code() {
        PEER_SHARE_LOCKED_CODE => (
            code::LOCKED,
            locked_error(code::LOCKED, "peer share store is locked"),
        ),
        PEER_SHARE_UNSUPPORTED_CODE => (
            code::UNSUPPORTED,
            ApiError {
                status: StatusCode::NOT_IMPLEMENTED,
                code: code::UNSUPPORTED.to_string(),
                message: "peer does not serve remote-share".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_UNSUPPORTED_REQUEST_CODE => (
            code::UNSUPPORTED_REQUEST,
            ApiError {
                status: StatusCode::NOT_IMPLEMENTED,
                code: code::UNSUPPORTED_REQUEST.to_string(),
                message: "peer does not implement this share operation".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_SUSPENDED_CODE => (
            code::SUSPENDED,
            conflict_with(code::SUSPENDED, "peer suspended all shares"),
        ),
        PEER_SHARE_REVOKED_CODE => (
            code::REVOKED,
            forbidden_with(code::REVOKED, "the share grant was revoked"),
        ),
        PEER_SHARE_DENIED_CODE => (
            code::DENIED,
            forbidden_with(code::DENIED, "the share grant denies this operation"),
        ),
        PEER_SHARE_RESERVED_NAME_CODE => (
            code::RESERVED_NAME,
            ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: code::RESERVED_NAME.to_string(),
                message: "the path uses a reserved name".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_NOT_FOUND_CODE => (
            code::NOT_FOUND,
            ApiError::not_found("share entry not found").with_code(code::NOT_FOUND),
        ),
        PEER_SHARE_CONFLICT_CODE => (
            code::CONFLICT,
            conflict_with(code::CONFLICT, "the target already exists"),
        ),
        PEER_SHARE_ROOT_GONE_CODE => (
            code::ROOT_GONE,
            ApiError {
                status: StatusCode::GONE,
                code: code::ROOT_GONE.to_string(),
                message: "the share root is gone".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_QUOTA_CODE => (
            code::QUOTA,
            ApiError {
                status: StatusCode::INSUFFICIENT_STORAGE,
                code: code::QUOTA.to_string(),
                message: "the peer's share quota is exhausted".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_TOO_LARGE_CODE => (
            code::TOO_LARGE,
            ApiError {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: code::TOO_LARGE.to_string(),
                message: "the transfer exceeds the peer's size limit".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_TOO_MANY_ENTRIES_CODE => (
            code::TOO_MANY_ENTRIES,
            ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: code::TOO_MANY_ENTRIES.to_string(),
                message: "the transfer has too many entries".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_IO_CODE => (
            code::IO,
            ApiError {
                status: StatusCode::BAD_GATEWAY,
                code: code::IO.to_string(),
                message: "the peer reported an I/O failure".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_CANCELLED_CODE => (
            code::CANCELLED,
            conflict_with(code::CANCELLED, "the transfer was cancelled"),
        ),
        PEER_SHARE_TOO_LATE_CODE => (
            code::TOO_LATE,
            conflict_with(
                code::TOO_LATE,
                "the transfer already reached its terminal state",
            ),
        ),
        PEER_SHARE_INTERRUPTED_CODE => (
            code::INTERRUPTED,
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: code::INTERRUPTED.to_string(),
                message: "the connection dropped mid-transfer".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_UNAVAILABLE_CODE => (
            code::UNAVAILABLE,
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: code::UNAVAILABLE.to_string(),
                message: "the peer is unreachable".to_string(),
                details: None,
            },
        ),
        PEER_SHARE_FAILED_CODE | REMOTE_SHARE_STATE_FAILED_CODE => (
            code::INTERNAL,
            ApiError::internal("remote-share operation failed").with_code(code::INTERNAL),
        ),
        _ => return share_category_fallback(op, error),
    };
    let api = if error.is_retryable() {
        api.with_details(json!({ "retryable": true }))
    } else {
        api
    };
    log_facade_failure("remote_share", op, variant, api.status, &api.message);
    api
}

/// Codes outside the remote-share block: fold on `category` so novel engine
/// failures still surface with a sane status instead of panicking.
fn share_category_fallback(op: &'static str, error: EngineError) -> ApiError {
    let api = match error.category() {
        EngineErrorCategory::InvalidInput => ApiError::bad_request("invalid remote-share request"),
        EngineErrorCategory::NotFound => ApiError::not_found("remote-share resource not found"),
        EngineErrorCategory::Conflict | EngineErrorCategory::InvalidState => {
            ApiError::conflict("remote-share state conflict")
        }
        EngineErrorCategory::Unauthorized => {
            ApiError::unauthorized("remote-share request was not authorized")
        }
        EngineErrorCategory::Unavailable => {
            ApiError::service_unavailable("remote-share service unavailable")
        }
        EngineErrorCategory::Cancelled => {
            ApiError::conflict("the remote-share operation was cancelled")
                .with_code(remote_share_code::CANCELLED)
        }
        EngineErrorCategory::DeadlineExceeded => ApiError {
            status: StatusCode::GATEWAY_TIMEOUT,
            code: "deadline_exceeded".to_string(),
            message: "remote-share operation timed out".to_string(),
            details: None,
        },
        EngineErrorCategory::Internal => ApiError::internal("remote-share operation failed"),
    };
    log_facade_failure(
        "remote_share",
        op,
        "category_fallback",
        api.status,
        &api.message,
    );
    api
}

fn locked_error(code: &'static str, message: &str) -> ApiError {
    ApiError {
        status: StatusCode::LOCKED,
        code: code.to_string(),
        message: message.to_string(),
        details: None,
    }
}

fn forbidden_with(code: &'static str, message: &str) -> ApiError {
    ApiError {
        status: StatusCode::FORBIDDEN,
        code: code.to_string(),
        message: message.to_string(),
        details: None,
    }
}

fn conflict_with(code: &'static str, message: &str) -> ApiError {
    ApiError {
        status: StatusCode::CONFLICT,
        code: code.to_string(),
        message: message.to_string(),
        details: None,
    }
}

/// Peer-scoped failure path: record availability transitions implied by the
/// denial (peer locked / suspended / unsupported) before translating to HTTP.
fn peer_error(
    state: &DaemonApiState,
    peer_id: &str,
    op: &'static str,
    error: EngineError,
) -> ApiError {
    let availability = match error.code() {
        PEER_SHARE_LOCKED_CODE => Some(PeerShareAvailability::Locked),
        PEER_SHARE_SUSPENDED_CODE => Some(PeerShareAvailability::Suspended),
        PEER_SHARE_UNSUPPORTED_CODE => Some(PeerShareAvailability::Unsupported),
        _ => None,
    };
    if let Some(availability) = availability {
        publish_availability(state, peer_id, availability);
    }
    share_error_to_api(op, error)
}

/// Publish `remote-share.availability_changed` on the shared `file-transfer`
/// topic when the observed state actually moved.
fn publish_availability(state: &DaemonApiState, peer_id: &str, state_value: PeerShareAvailability) {
    if !state.remote_share.observe_peer_state(peer_id, state_value) {
        return;
    }
    let wire = match state_value {
        PeerShareAvailability::Ok => "ok",
        PeerShareAvailability::Locked => "locked",
        PeerShareAvailability::Unsupported => "unsupported",
        PeerShareAvailability::Suspended => "suspended",
    };
    let _ = state.event_tx.send(DaemonWsEvent {
        topic: ws_topic::FILE_TRANSFER.to_string(),
        event_type: ws_event::REMOTE_SHARE_AVAILABILITY_CHANGED.to_string(),
        session_id: None,
        ts: chrono::Utc::now().timestamp_millis(),
        payload: json!({ "peerId": peer_id, "state": wire }),
    });
}

// ============================================================================
// Query params
// ============================================================================

/// `GET /peers/{peerId}/share/list`: no `root` → root list; with `root` → one
/// directory page of `?dir` (empty = root top level).
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareListQuery {
    /// Share-root id. Absent → root list.
    pub root: Option<String>,
    /// Root-relative directory path (default: root top level).
    #[serde(default)]
    pub dir: Option<String>,
    /// Resume cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Page size (default 2000 — the client-side cap, spec §11.3).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// `GET /peers/{peerId}/share/stat`: `?root&rel`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareStatQuery {
    pub root: String,
    /// Root-relative path of the entry.
    pub rel: String,
}

/// `GET /peers/{peerId}/share/trash`: `?root&cursor&limit`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareTrashQuery {
    pub root: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Client-side page-size cap is 2000 (spec §11.3); the holder's 2 MiB wire cap
/// may shrink a page further, signalled via `truncated`.
const DEFAULT_PAGE_LIMIT: u32 = 2000;

fn page_limit(limit: Option<u32>) -> u32 {
    limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, DEFAULT_PAGE_LIMIT)
}

// ============================================================================
// DTO conversion
// ============================================================================

fn remote_root_to_dto(root: &RemoteShareRootSummary) -> RemoteShareRootDto {
    RemoteShareRootDto {
        id: root.id.clone(),
        path: root.path.to_string_lossy().into_owned(),
        label: root.label.clone(),
        read: root.read,
        write: root.write,
        delete: root.delete,
        enabled: root.enabled,
    }
}

fn availability_of(state: PeerShareState) -> PeerShareAvailability {
    match state {
        PeerShareState::Ok => PeerShareAvailability::Ok,
        PeerShareState::Locked => PeerShareAvailability::Locked,
        PeerShareState::Unsupported => PeerShareAvailability::Unsupported,
        PeerShareState::Suspended => PeerShareAvailability::Suspended,
    }
}

// ============================================================================
// Router
// ============================================================================

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route(http_route::REMOTE_SHARE_STATE, get(get_remote_share_state))
        .route(http_route::REMOTE_SHARE_ROOTS, put(put_remote_share_roots))
        .route(
            http_route::REMOTE_SHARE_SUSPEND,
            post(post_remote_share_suspend),
        )
        .route(
            http_route::REMOTE_SHARE_TRANSFER_CANCEL,
            post(post_transfer_cancel),
        )
        .route(http_route::PEER_SHARE_STATE, get(get_peer_share_state))
        .route(http_route::PEER_SHARE_LIST, get(get_peer_share_list))
        .route(http_route::PEER_SHARE_STAT, get(get_peer_share_stat))
        .route(http_route::PEER_SHARE_TRASH, get(get_peer_share_trash))
        .route(
            http_route::PEER_SHARE_TRASH_RESTORE,
            post(post_peer_share_restore),
        )
        .route(http_route::PEER_SHARE_UPLOAD, post(post_peer_share_upload))
        .route(
            http_route::PEER_SHARE_DOWNLOAD,
            post(post_peer_share_download),
        )
        .route(http_route::PEER_SHARE_MKDIR, post(post_peer_share_mkdir))
        .route(http_route::PEER_SHARE_RENAME, post(post_peer_share_rename))
        .route(http_route::PEER_SHARE_DELETE, post(post_peer_share_delete))
        .route(http_route::PEER_SHARE_COPY, post(post_peer_share_copy))
        .route(
            http_route::PEER_SHARE_EMPTY_TRASH,
            post(post_peer_share_empty_trash),
        )
}

// ============================================================================
// Local share management
// ============================================================================

/// GET /remote-share/state — local share state; locked shape `200` while the
/// session is absent (spec §9).
#[utoipa::path(
    get,
    path = "/remote-share/state",
    operation_id = "getRemoteShareState",
    tag = "remote-share",
    responses(
        (status = 200, description = "Local share state (locked shape while locked)", body = RemoteShareStateEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_remote_share_state(
    State(state): State<DaemonApiState>,
) -> Result<Json<RemoteShareStateEnvelope>, ApiError> {
    async move {
        match state.execute(Operation::QueryRemoteShareState).await {
            Ok(OperationResult::RemoteShareState(RemoteShareStateSummary { suspended, roots })) => {
                Ok(Json(ApiEnvelope::now(RemoteShareStateDto {
                    locked: false,
                    suspended: Some(suspended),
                    roots: Some(roots.iter().map(remote_root_to_dto).collect()),
                })))
            }
            Ok(_) => Err(ApiError::internal(
                "engine returned an unexpected remote-share state result",
            )),
            Err(error) if is_session_locked(&error) => {
                Ok(Json(ApiEnvelope::now(RemoteShareStateDto {
                    locked: true,
                    suspended: None,
                    roots: None,
                })))
            }
            Err(error) => Err(share_error_to_api("query_remote_share_state", error)),
        }
    }
    .instrument(info_span!("api.remote_share.state"))
    .await
}

/// PUT /remote-share/roots — replace the whole whitelist (423 while locked).
#[utoipa::path(
    put,
    path = "/remote-share/roots",
    operation_id = "replaceRemoteShareRoots",
    tag = "remote-share",
    request_body = ReplaceRemoteShareRootsRequest,
    responses(
        (status = 200, description = "Whitelist reconciled", body = RemoteShareReconcileEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn put_remote_share_roots(
    State(state): State<DaemonApiState>,
    Json(req): Json<ReplaceRemoteShareRootsRequest>,
) -> Result<Json<RemoteShareReconcileEnvelope>, ApiError> {
    let span = info_span!(
        "api.remote_share.replace_roots",
        root_count = req.roots.len()
    );
    async move {
        let roots = req
            .roots
            .into_iter()
            .map(|root: RemoteShareRootInputDto| RemoteShareRootInput {
                id: root.id,
                path: PathBuf::from(root.path),
                label: root.label,
                read: root.read,
                write: root.write,
                delete: root.delete,
                enabled: root.enabled,
            })
            .collect();
        let result = state
            .execute(Operation::ReplaceRemoteShareRoots(
                ReplaceRemoteShareRootsInput { roots },
            ))
            .await
            .map_err(|e| share_error_to_api("replace_remote_share_roots", e))?;
        let OperationResult::RemoteShareReconciled(summary) = result else {
            return Err(ApiError::internal(
                "engine returned an unexpected remote-share reconcile result",
            ));
        };
        Ok(Json(ApiEnvelope::now(reconcile_to_dto(summary))))
    }
    .instrument(span)
    .await
}

fn reconcile_to_dto(summary: RemoteShareReconcileSummary) -> RemoteShareReconcileDto {
    RemoteShareReconcileDto {
        registered: summary.registered,
        failed: summary.failed,
        unregistered: summary.unregistered,
    }
}

/// POST /remote-share/suspend — suspend or resume every share root.
#[utoipa::path(
    post,
    path = "/remote-share/suspend",
    operation_id = "setRemoteShareSuspended",
    tag = "remote-share",
    request_body = SetRemoteShareSuspendedRequest,
    responses(
        (status = 200, description = "Suspend state reconciled", body = RemoteShareReconcileEnvelope),
        (status = 423, description = "Encryption session locked", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn post_remote_share_suspend(
    State(state): State<DaemonApiState>,
    Json(req): Json<SetRemoteShareSuspendedRequest>,
) -> Result<Json<RemoteShareReconcileEnvelope>, ApiError> {
    async move {
        let result = state
            .execute(Operation::SetRemoteShareSuspended(
                SetRemoteShareSuspendedInput {
                    suspended: req.suspended,
                },
            ))
            .await
            .map_err(|e| share_error_to_api("set_remote_share_suspended", e))?;
        let OperationResult::RemoteShareReconciled(summary) = result else {
            return Err(ApiError::internal(
                "engine returned an unexpected remote-share suspend result",
            ));
        };
        Ok(Json(ApiEnvelope::now(reconcile_to_dto(summary))))
    }
    .instrument(info_span!("api.remote_share.suspend"))
    .await
}

/// POST /remote-share/transfers/:transfer_id/cancel — cancel an in-flight
/// remote-share upload or download.
#[utoipa::path(
    post,
    path = "/remote-share/transfers/{transfer_id}/cancel",
    operation_id = "cancelRemoteShareTransfer",
    tag = "remote-share",
    params(("transfer_id" = String, Path, description = "Caller-issued transfer id")),
    responses(
        (status = 200, description = "Cancel outcome", body = PeerShareTransferCancelEnvelope),
        (status = 423, description = "Encryption session locked", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn post_transfer_cancel(
    State(state): State<DaemonApiState>,
    AxumPath(transfer_id): AxumPath<String>,
) -> Result<Json<PeerShareTransferCancelEnvelope>, ApiError> {
    let span = info_span!("api.remote_share.cancel_transfer");
    async move {
        let result = state
            .execute(Operation::CancelPeerShareTransfer(
                CancelPeerShareTransferInput {
                    transfer_id: transfer_id.clone(),
                },
            ))
            .await
            .map_err(|e| share_error_to_api("cancel_peer_share_transfer", e))?;
        let OperationResult::PeerShareTransferCancellation(outcome) = result else {
            return Err(ApiError::internal(
                "engine returned an unexpected remote-share cancel result",
            ));
        };
        // Whatever the outcome, no terminal event for this id is still owed to
        // the frontend: cancel accepted / already terminal / never seen.
        state.remote_share.finish(&transfer_id);
        let outcome = match outcome {
            uc_engine::PeerShareTransferCancelOutcome::Cancelled => {
                PeerShareTransferCancelOutcomeDto::Cancelled
            }
            uc_engine::PeerShareTransferCancelOutcome::NotInflight => {
                PeerShareTransferCancelOutcomeDto::NotInflight
            }
            uc_engine::PeerShareTransferCancelOutcome::TooLate => {
                PeerShareTransferCancelOutcomeDto::TooLate
            }
        };
        Ok(Json(ApiEnvelope::now(PeerShareTransferCancelResultDto {
            outcome,
        })))
    }
    .instrument(span)
    .await
}

// ============================================================================
// Peer side: browse
// ============================================================================

/// GET /peers/:peer_id/share/state — peer share availability (Hello).
#[utoipa::path(
    get,
    path = "/peers/{peer_id}/share/state",
    operation_id = "getPeerShareState",
    tag = "remote-share",
    params(("peer_id" = String, Path, description = "Peer device id")),
    responses(
        (status = 200, description = "Peer share availability", body = PeerShareStateEnvelope),
        (status = 423, description = "Encryption session locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_peer_share_state(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
) -> Result<Json<PeerShareStateEnvelope>, ApiError> {
    let span = info_span!("api.remote_share.peer_state", peer_id = %peer_id);
    async move {
        let result = state
            .execute(Operation::QueryPeerShareState(PeerShareRequest {
                peer_id: peer_id.clone(),
            }))
            .await
            .map_err(|e| peer_error(&state, &peer_id, "query_peer_share_state", e))?;
        let OperationResult::PeerShareState(peer_state) = result else {
            return Err(ApiError::internal(
                "engine returned an unexpected peer-share state result",
            ));
        };
        let availability = availability_of(peer_state);
        publish_availability(&state, &peer_id, availability);
        Ok(Json(ApiEnvelope::now(PeerShareStateDto {
            state: availability,
        })))
    }
    .instrument(span)
    .await
}

/// GET /peers/:peer_id/share/list — root list or one directory page.
#[utoipa::path(
    get,
    path = "/peers/{peer_id}/share/list",
    operation_id = "listPeerShare",
    tag = "remote-share",
    params(
        ("peer_id" = String, Path, description = "Peer device id"),
        PeerShareListQuery,
    ),
    responses(
        (status = 200, description = "Root list or directory page", body = PeerShareListEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 403, description = "Denied by the peer", body = ApiErrorResponse),
        (status = 404, description = "Root or entry not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_peer_share_list(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
    Query(query): Query<PeerShareListQuery>,
) -> Result<Json<PeerShareListEnvelope>, ApiError> {
    let span = info_span!("api.remote_share.list", peer_id = %peer_id);
    async move {
        match query.root {
            None => {
                let result = state
                    .execute(Operation::ListPeerShareRoots(PeerShareRequest {
                        peer_id: peer_id.clone(),
                    }))
                    .await
                    .map_err(|e| peer_error(&state, &peer_id, "list_peer_share_roots", e))?;
                let OperationResult::PeerShareRoots(roots) = result else {
                    return Err(ApiError::internal(
                        "engine returned an unexpected peer-share roots result",
                    ));
                };
                Ok(Json(ApiEnvelope::now(PeerShareListResponse::Roots {
                    roots: roots
                        .iter()
                        .map(|root| PeerShareRootDto {
                            id: root.id.clone(),
                            label: root.label.clone(),
                            read: root.read,
                            write: root.write,
                            delete: root.delete,
                            enabled: root.enabled,
                        })
                        .collect(),
                })))
            }
            Some(root_id) => {
                let result = state
                    .execute(Operation::ListPeerShareDirectory(PeerShareListInput {
                        peer_id: peer_id.clone(),
                        root_id,
                        rel: query.dir.unwrap_or_default(),
                        after: query.cursor,
                        limit: page_limit(query.limit),
                    }))
                    .await
                    .map_err(|e| peer_error(&state, &peer_id, "list_peer_share_directory", e))?;
                let OperationResult::PeerShareDirectory(page) = result else {
                    return Err(ApiError::internal(
                        "engine returned an unexpected peer-share directory result",
                    ));
                };
                Ok(Json(ApiEnvelope::now(PeerShareListResponse::Directory {
                    entries: page.entries.iter().map(entry_to_dto).collect(),
                    cursor: page.cursor,
                    truncated: page.truncated,
                })))
            }
        }
    }
    .instrument(span)
    .await
}

fn entry_to_dto(entry: &uc_engine::PeerShareEntrySummary) -> PeerShareEntryDto {
    PeerShareEntryDto {
        name: entry.name.clone(),
        is_dir: entry.is_dir,
        is_symlink: entry.is_symlink,
        size_bytes: entry.size_bytes,
        modified_at_ms: entry.modified_at_ms,
    }
}

/// GET /peers/:peer_id/share/stat — single entry metadata.
#[utoipa::path(
    get,
    path = "/peers/{peer_id}/share/stat",
    operation_id = "statPeerShareEntry",
    tag = "remote-share",
    params(
        ("peer_id" = String, Path, description = "Peer device id"),
        PeerShareStatQuery,
    ),
    responses(
        (status = 200, description = "Entry metadata", body = PeerShareEntryEnvelope),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_peer_share_stat(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
    Query(query): Query<PeerShareStatQuery>,
) -> Result<Json<PeerShareEntryEnvelope>, ApiError> {
    let span = info_span!("api.remote_share.stat", peer_id = %peer_id);
    async move {
        let result = state
            .execute(Operation::StatPeerShareEntry(PeerShareRelInput {
                peer_id: peer_id.clone(),
                root_id: query.root,
                rel: query.rel,
            }))
            .await
            .map_err(|e| peer_error(&state, &peer_id, "stat_peer_share_entry", e))?;
        let OperationResult::PeerShareEntry(entry) = result else {
            return Err(ApiError::internal(
                "engine returned an unexpected peer-share stat result",
            ));
        };
        Ok(Json(ApiEnvelope::now(entry_to_dto(&entry))))
    }
    .instrument(span)
    .await
}

/// GET /peers/:peer_id/share/trash — trash listing page.
#[utoipa::path(
    get,
    path = "/peers/{peer_id}/share/trash",
    operation_id = "listPeerShareTrash",
    tag = "remote-share",
    params(
        ("peer_id" = String, Path, description = "Peer device id"),
        PeerShareTrashQuery,
    ),
    responses(
        (status = 200, description = "Trash page", body = PeerShareTrashPageEnvelope),
        (status = 404, description = "Root not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_peer_share_trash(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
    Query(query): Query<PeerShareTrashQuery>,
) -> Result<Json<PeerShareTrashPageEnvelope>, ApiError> {
    let span = info_span!("api.remote_share.trash", peer_id = %peer_id);
    async move {
        let result = state
            .execute(Operation::ListPeerShareTrash(PeerShareTrashListInput {
                peer_id: peer_id.clone(),
                root_id: query.root,
                after: query.cursor,
                limit: page_limit(query.limit),
            }))
            .await
            .map_err(|e| peer_error(&state, &peer_id, "list_peer_share_trash", e))?;
        let OperationResult::PeerShareTrash(page) = result else {
            return Err(ApiError::internal(
                "engine returned an unexpected peer-share trash result",
            ));
        };
        Ok(Json(ApiEnvelope::now(PeerShareTrashPageDto {
            entries: page
                .entries
                .iter()
                .map(
                    |entry: &uc_engine::PeerShareTrashEntrySummary| PeerShareTrashEntryDto {
                        trash_id: entry.trash_id.clone(),
                        original_rel: entry.original_rel.clone(),
                        deleted_at_ms: entry.deleted_at_ms,
                        is_dir: entry.is_dir,
                        size_bytes: entry.size_bytes,
                    },
                )
                .collect(),
            cursor: page.cursor,
            truncated: page.truncated,
        })))
    }
    .instrument(span)
    .await
}

// ============================================================================
// Peer side: mutations
// ============================================================================

/// Shared tail of every mutation handler: dispatch `MutatePeerShare` and map
/// `PeerShareMutated` onto the wire result.
async fn run_peer_share_mutation(
    state: &DaemonApiState,
    peer_id: String,
    root_id: String,
    action: PeerShareAction,
    op: &'static str,
) -> Result<Json<PeerShareMutationResultEnvelope>, ApiError> {
    let result = state
        .execute(Operation::MutatePeerShare(MutatePeerShareInput {
            peer_id: peer_id.clone(),
            root_id,
            action,
        }))
        .await
        .map_err(|e| peer_error(state, &peer_id, op, e))?;
    let OperationResult::PeerShareMutated(outcome) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected peer-share mutation result",
        ));
    };
    let dto = match outcome {
        uc_engine::PeerShareMutationOutcome::Ack => PeerShareMutationResultDto::Ack,
        uc_engine::PeerShareMutationOutcome::Deleted { trash_id } => {
            PeerShareMutationResultDto::Deleted { trash_id }
        }
    };
    Ok(Json(ApiEnvelope::now(dto)))
}

macro_rules! peer_share_mutation_handler {
    ($fn_name:ident, $route:literal, $op_id:literal, $op:literal, $body:ty, $action:expr, $summary:literal) => {
        #[utoipa::path(
            post,
            path = $route,
            operation_id = $op_id,
            tag = "remote-share",
            params(("peer_id" = String, Path, description = "Peer device id")),
            request_body = $body,
            responses(
                (status = 200, description = "Mutation outcome", body = PeerShareMutationResultEnvelope),
                (status = 403, description = "Denied by the peer", body = ApiErrorResponse),
                (status = 404, description = "Entry not found", body = ApiErrorResponse),
                (status = 409, description = "Conflict", body = ApiErrorResponse),
                (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
                (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
                (status = 500, description = "Internal server error", body = ApiErrorResponse),
            )
        )]
        async fn $fn_name(
            State(state): State<DaemonApiState>,
            AxumPath(peer_id): AxumPath<String>,
            Json(req): Json<$body>,
        ) -> Result<Json<PeerShareMutationResultEnvelope>, ApiError> {
            let span = info_span!($op, peer_id = %peer_id);
            let action = ($action)(&req);
            async move {
                run_peer_share_mutation(&state, peer_id, req.root.clone(), action, $op).await
            }
            .instrument(span)
            .await
        }
    };
}

peer_share_mutation_handler!(
    post_peer_share_mkdir,
    "/peers/{peer_id}/share/mkdir",
    "mkdirPeerShare",
    "api.remote_share.mkdir",
    PeerShareMkdirRequest,
    |req: &PeerShareMkdirRequest| PeerShareAction::Mkdir {
        rel: req.rel.clone()
    },
    "create a directory"
);

peer_share_mutation_handler!(
    post_peer_share_rename,
    "/peers/{peer_id}/share/rename",
    "renamePeerShare",
    "api.remote_share.rename",
    PeerShareRenameRequest,
    |req: &PeerShareRenameRequest| PeerShareAction::Rename {
        from: req.from.clone(),
        to: req.to.clone()
    },
    "rename an entry"
);

peer_share_mutation_handler!(
    post_peer_share_delete,
    "/peers/{peer_id}/share/delete",
    "deletePeerShareEntry",
    "api.remote_share.delete",
    PeerShareDeleteRequest,
    |req: &PeerShareDeleteRequest| PeerShareAction::Delete {
        rel: req.rel.clone()
    },
    "delete an entry into the peer's trash"
);

peer_share_mutation_handler!(
    post_peer_share_copy,
    "/peers/{peer_id}/share/copy",
    "copyPeerShareEntry",
    "api.remote_share.copy",
    PeerShareCopyRequest,
    |req: &PeerShareCopyRequest| PeerShareAction::CopyWithin {
        from: req.from.clone(),
        to: req.to.clone()
    },
    "copy an entry within a root"
);

peer_share_mutation_handler!(
    post_peer_share_empty_trash,
    "/peers/{peer_id}/share/empty-trash",
    "emptyPeerShareTrash",
    "api.remote_share.empty_trash",
    PeerShareEmptyTrashRequest,
    |_req: &PeerShareEmptyTrashRequest| PeerShareAction::EmptyTrash,
    "empty a root's trash"
);

/// POST /peers/:peer_id/share/trash/restore — restore a trash entry.
#[utoipa::path(
    post,
    path = "/peers/{peer_id}/share/trash/restore",
    operation_id = "restorePeerShareTrash",
    tag = "remote-share",
    params(("peer_id" = String, Path, description = "Peer device id")),
    request_body = PeerShareRestoreRequest,
    responses(
        (status = 200, description = "Mutation outcome", body = PeerShareMutationResultEnvelope),
        (status = 403, description = "Denied by the peer", body = ApiErrorResponse),
        (status = 404, description = "Trash entry not found", body = ApiErrorResponse),
        (status = 409, description = "Conflict", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn post_peer_share_restore(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
    Json(req): Json<PeerShareRestoreRequest>,
) -> Result<Json<PeerShareMutationResultEnvelope>, ApiError> {
    let span = info_span!("api.remote_share.restore", peer_id = %peer_id);
    async move {
        run_peer_share_mutation(
            &state,
            peer_id,
            req.root.clone(),
            PeerShareAction::Restore {
                trash_id: req.trash_id.clone(),
                to: req.to.clone(),
            },
            "api.remote_share.restore",
        )
        .await
    }
    .instrument(span)
    .await
}

// ============================================================================
// Peer side: transfers (async — POST returns the transfer id immediately)
// ============================================================================

/// Spawn an upload/download op on a background task so the POST can return the
/// caller-issued `transfer_id` at once (spec §8.3). If `execute` fails *before*
/// the engine accepted the transfer no terminal event will arrive, so the task
/// synthesizes one — `tracker.finish` collapses it against a racing real
/// terminal.
fn spawn_transfer(
    state: &DaemonApiState,
    transfer_id: String,
    op: Operation,
    op_name: &'static str,
) {
    let engine = Arc::clone(&state.engine);
    let tracker = state.remote_share.clone();
    let event_tx = state.event_tx.clone();
    tracker.begin(&transfer_id);
    tokio::spawn(async move {
        if let Err(error) = engine.execute(op).await {
            warn!(
                op = op_name,
                code = error.code(),
                category = %error.category(),
                "remote-share transfer failed before completion"
            );
            if tracker.finish(&transfer_id) {
                let _ = event_tx.send(DaemonWsEvent {
                    topic: ws_topic::FILE_TRANSFER.to_string(),
                    event_type: ws_event::REMOTE_SHARE_STATUS_CHANGED.to_string(),
                    session_id: None,
                    ts: chrono::Utc::now().timestamp_millis(),
                    payload: json!({
                        "transferId": transfer_id,
                        "status": "failed",
                        "reason": error_code_label(error.code()),
                    }),
                });
            }
        }
    });
}

/// Stable label for a synthesized terminal failure (matches the API code for
/// the same engine numeric code; unknown codes keep the numeric value).
fn error_code_label(code: u32) -> String {
    use remote_share_code as api;
    let label = match code {
        PEER_SHARE_LOCKED_CODE => api::LOCKED,
        PEER_SHARE_UNSUPPORTED_CODE => api::UNSUPPORTED,
        PEER_SHARE_UNSUPPORTED_REQUEST_CODE => api::UNSUPPORTED_REQUEST,
        PEER_SHARE_SUSPENDED_CODE => api::SUSPENDED,
        PEER_SHARE_REVOKED_CODE => api::REVOKED,
        PEER_SHARE_DENIED_CODE => api::DENIED,
        PEER_SHARE_RESERVED_NAME_CODE => api::RESERVED_NAME,
        PEER_SHARE_NOT_FOUND_CODE => api::NOT_FOUND,
        PEER_SHARE_CONFLICT_CODE => api::CONFLICT,
        PEER_SHARE_ROOT_GONE_CODE => api::ROOT_GONE,
        PEER_SHARE_QUOTA_CODE => api::QUOTA,
        PEER_SHARE_TOO_LARGE_CODE => api::TOO_LARGE,
        PEER_SHARE_TOO_MANY_ENTRIES_CODE => api::TOO_MANY_ENTRIES,
        PEER_SHARE_IO_CODE => api::IO,
        PEER_SHARE_CANCELLED_CODE => api::CANCELLED,
        PEER_SHARE_TOO_LATE_CODE => api::TOO_LATE,
        PEER_SHARE_INTERRUPTED_CODE => api::INTERRUPTED,
        PEER_SHARE_UNAVAILABLE_CODE => api::UNAVAILABLE,
        _ => api::INTERNAL,
    };
    label.to_string()
}

/// POST /peers/:peer_id/share/upload — start an upload; `202 { transferId }`.
#[utoipa::path(
    post,
    path = "/peers/{peer_id}/share/upload",
    operation_id = "uploadPeerShare",
    tag = "remote-share",
    params(("peer_id" = String, Path, description = "Peer device id")),
    request_body = PeerShareUploadRequest,
    responses(
        (status = 202, description = "Upload accepted; track via remote-share.* events", body = PeerShareTransferStartedEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 403, description = "Denied by the peer", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn post_peer_share_upload(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
    Json(req): Json<PeerShareUploadRequest>,
) -> Result<(StatusCode, Json<PeerShareTransferStartedEnvelope>), ApiError> {
    let span = info_span!(
        "api.remote_share.upload",
        peer_id = %peer_id,
        source_count = req.sources.len(),
    );
    async move {
        if req.sources.is_empty() {
            return Err(ApiError::bad_request("at least one source is required"));
        }
        // `register_input` proves readability; the engine consumes `sources`
        // as plain filesystem paths, so the handles must carry the real
        // paths, not the opaque registry ids.
        let mut sources = Vec::with_capacity(req.sources.len());
        for source in &req.sources {
            state
                .file_handles
                .register_input(Path::new(source))
                .map_err(|_| ApiError::bad_request("a source path is not readable"))?;
            sources.push(uc_engine::HostFileHandle::new(source.clone()));
        }
        let transfer_id = uuid::Uuid::new_v4().to_string();
        spawn_transfer(
            &state,
            transfer_id.clone(),
            Operation::UploadPeerShare(UploadPeerShareInput {
                peer_id,
                root_id: req.root,
                dest_dir: req.dest_dir.unwrap_or_default(),
                transfer_id: transfer_id.clone(),
                sources,
            }),
            "upload_peer_share",
        );
        Ok((
            StatusCode::ACCEPTED,
            Json(ApiEnvelope::now(PeerShareTransferStartedDto {
                transfer_id,
            })),
        ))
    }
    .instrument(span)
    .await
}

/// POST /peers/:peer_id/share/download — start a download; `202 { transferId }`.
#[utoipa::path(
    post,
    path = "/peers/{peer_id}/share/download",
    operation_id = "downloadPeerShare",
    tag = "remote-share",
    params(("peer_id" = String, Path, description = "Peer device id")),
    request_body = PeerShareDownloadRequest,
    responses(
        (status = 202, description = "Download accepted; track via remote-share.* events", body = PeerShareTransferStartedEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 403, description = "Denied by the peer", body = ApiErrorResponse),
        (status = 423, description = "Encryption session locked or peer locked", body = ApiErrorResponse),
        (status = 503, description = "Peer unreachable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn post_peer_share_download(
    State(state): State<DaemonApiState>,
    AxumPath(peer_id): AxumPath<String>,
    Json(req): Json<PeerShareDownloadRequest>,
) -> Result<(StatusCode, Json<PeerShareTransferStartedEnvelope>), ApiError> {
    let span = info_span!("api.remote_share.download", peer_id = %peer_id);
    async move {
        // `register_output` proves writability and creates the placeholder;
        // the engine consumes `target` as a plain filesystem path (like
        // `FetchBlobToPathCommand::target_path`), so the handle must carry
        // the real path, not the opaque registry id.
        state
            .file_handles
            .register_output(Path::new(&req.target))
            .map_err(|_| ApiError::bad_request("the target path is not writable"))?;
        let target = uc_engine::HostFileHandle::new(req.target.clone());
        let transfer_id = uuid::Uuid::new_v4().to_string();
        spawn_transfer(
            &state,
            transfer_id.clone(),
            Operation::DownloadPeerShare(DownloadPeerShareInput {
                peer_id,
                root_id: req.root,
                rel: req.rel,
                transfer_id: transfer_id.clone(),
                target,
            }),
            "download_peer_share",
        );
        Ok((
            StatusCode::ACCEPTED,
            Json(ApiEnvelope::now(PeerShareTransferStartedDto {
                transfer_id,
            })),
        ))
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_engine::{EngineError, EngineErrorCategory};

    /// The spec §12 wire codes are one-to-one with the engine's numeric deny
    /// block — pin the whole translation table so an engine-side renumbering
    /// can never silently shift the API surface.
    #[test]
    fn deny_block_maps_one_to_one() {
        let cases: &[(u32, u16, &str)] = &[
            (PEER_SHARE_LOCKED_CODE, 423, "locked"),
            (PEER_SHARE_UNSUPPORTED_CODE, 501, "unsupported"),
            (
                PEER_SHARE_UNSUPPORTED_REQUEST_CODE,
                501,
                "unsupported_request",
            ),
            (PEER_SHARE_SUSPENDED_CODE, 409, "suspended"),
            (PEER_SHARE_REVOKED_CODE, 403, "revoked"),
            (PEER_SHARE_DENIED_CODE, 403, "denied"),
            (PEER_SHARE_RESERVED_NAME_CODE, 422, "reserved_name"),
            (PEER_SHARE_NOT_FOUND_CODE, 404, "not_found"),
            (PEER_SHARE_CONFLICT_CODE, 409, "conflict"),
            (PEER_SHARE_ROOT_GONE_CODE, 410, "root_gone"),
            (PEER_SHARE_QUOTA_CODE, 507, "quota"),
            (PEER_SHARE_TOO_LARGE_CODE, 413, "too_large"),
            (PEER_SHARE_TOO_MANY_ENTRIES_CODE, 422, "too_many_entries"),
            (PEER_SHARE_IO_CODE, 502, "io"),
            (PEER_SHARE_CANCELLED_CODE, 409, "cancelled"),
            (PEER_SHARE_TOO_LATE_CODE, 409, "too_late"),
            (PEER_SHARE_INTERRUPTED_CODE, 503, "interrupted"),
            (PEER_SHARE_UNAVAILABLE_CODE, 503, "unavailable"),
            (PEER_SHARE_FAILED_CODE, 500, "internal"),
            (REMOTE_SHARE_STATE_FAILED_CODE, 500, "internal"),
        ];
        for (engine_code, status, api_code) in cases {
            let err = EngineError::new(*engine_code, EngineErrorCategory::Internal, false);
            let api = share_error_to_api("test", err);
            assert_eq!(
                (api.status.as_u16(), api.code.as_str()),
                (*status, *api_code),
                "engine code {engine_code} must map to {status}/{api_code}"
            );
        }
    }

    #[test]
    fn session_absent_and_manifest_locked_both_become_423() {
        for code in [OPERATION_UNAVAILABLE_CODE, REMOTE_SHARE_LOCKED_CODE] {
            let err = EngineError::new(code, EngineErrorCategory::Unavailable, false);
            let api = share_error_to_api("test", err);
            assert_eq!(api.status, StatusCode::LOCKED);
            assert_eq!(api.code, "session_locked");
        }
    }

    #[test]
    fn retryable_is_threaded_into_details() {
        let err = EngineError::new(
            PEER_SHARE_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        );
        let api = share_error_to_api("test", err);
        assert_eq!(api.details, Some(json!({ "retryable": true })));
    }

    #[test]
    fn tracker_terminal_collapses_duplicates() {
        let tracker = RemoteShareTracker::default();
        tracker.begin("t-1");
        assert_eq!(tracker.mark("t-1"), RemoteShareTransferMark::Inflight);
        assert!(tracker.finish("t-1"));
        assert_eq!(tracker.mark("t-1"), RemoteShareTransferMark::Finished);
        assert!(!tracker.finish("t-1"));
        assert_eq!(tracker.mark("other"), RemoteShareTransferMark::Unknown);
    }

    #[test]
    fn peer_state_first_observation_is_baseline() {
        let tracker = RemoteShareTracker::default();
        assert!(!tracker.observe_peer_state("p", PeerShareAvailability::Ok));
        assert!(!tracker.observe_peer_state("p", PeerShareAvailability::Ok));
        assert!(tracker.observe_peer_state("p", PeerShareAvailability::Locked));
        assert!(!tracker.observe_peer_state("p", PeerShareAvailability::Locked));
    }
}
