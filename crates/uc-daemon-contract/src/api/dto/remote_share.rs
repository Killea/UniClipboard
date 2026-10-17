//! DTOs for the remote-share (device file share) loopback endpoints.
//!
//! Wire shape follows spec `docs/specs/2026-10-12-device-file-share-spec.md`
//! §9/§12: every field is camelCase and the stable error `code` strings live
//! in [`crate::constants::remote_share_code`]. Domain → DTO conversion lives
//! in `uc-webserver` (this crate does not depend on `uc-engine`).
//!
//! `path` on the *local* management DTOs is a real host path: it is served
//! only on the authenticated loopback API so the settings UI can render and
//! edit the whitelist — it never crosses the P2P wire (spec §13 I2).
//! `root`/`rel`/`label`/`trashId`/`name` are protocol content, not host paths.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ── Local share management ──────────────────────────────────────────────────

/// One authorized share root as the local owner sees it.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoteShareRootDto {
    /// Opaque root id assigned by the engine (stable across restarts).
    pub id: String,
    /// Absolute host path of the shared directory (local API only).
    pub path: String,
    /// Label the peer sees in its directory tree.
    pub label: String,
    pub read: bool,
    pub write: bool,
    pub delete: bool,
    pub enabled: bool,
}

/// `GET /remote-share/state` response. While the encryption session is locked
/// the route returns 200 with `locked: true` and null `suspended`/`roots` so
/// the settings page can render "unlock to manage" without exposing paths.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoteShareStateDto {
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspended: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<RemoteShareRootDto>>,
}

/// One entry of the `PUT /remote-share/roots` whitelist write. `id` absent =
/// new root (the engine assigns one).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoteShareRootInputDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Absolute host path of the directory to share.
    pub path: String,
    pub label: String,
    pub read: bool,
    pub write: bool,
    pub delete: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Request body for `PUT /remote-share/roots` — replaces the whole whitelist.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRemoteShareRootsRequest {
    pub roots: Vec<RemoteShareRootInputDto>,
}

/// Request body for `POST /remote-share/suspend`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetRemoteShareSuspendedRequest {
    pub suspended: bool,
}

/// Reconcile summary shared by `PUT /remote-share/roots` and
/// `POST /remote-share/suspend`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoteShareReconcileDto {
    pub registered: u32,
    pub failed: u32,
    pub unregistered: u32,
}

// ── Peer side: browse ───────────────────────────────────────────────────────

/// Peer share-service availability (wire `Hello.state` mirror).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerShareAvailability {
    Ok,
    Locked,
    Unsupported,
    Suspended,
}

/// `GET /peers/{peerId}/share/state` response.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareStateDto {
    pub state: PeerShareAvailability,
}

/// A share root the peer exposes to us (no host path — spec §13 I2).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareRootDto {
    pub id: String,
    pub label: String,
    pub read: bool,
    pub write: bool,
    pub delete: bool,
    pub enabled: bool,
}

/// One directory entry inside a peer share root.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareEntryDto {
    /// Single path segment.
    pub name: String,
    pub is_dir: bool,
    /// Listed faithfully but not addressable (the holder's confinement layer
    /// refuses every verb on it).
    pub is_symlink: bool,
    pub size_bytes: u64,
    pub modified_at_ms: i64,
}

/// `GET /peers/{peerId}/share/list` response: without `root` it is the root
/// list; with `root` it is one directory page.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PeerShareListResponse {
    /// Root list (`kind: "roots"`).
    Roots { roots: Vec<PeerShareRootDto> },
    /// One directory page (`kind: "directory"`).
    Directory {
        entries: Vec<PeerShareEntryDto>,
        /// Resume cursor for the next page (`after` on the next request).
        cursor: Option<String>,
        truncated: bool,
    },
}

/// One trash entry inside a peer share root.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareTrashEntryDto {
    pub trash_id: String,
    /// Root-relative path the entry had before deletion.
    pub original_rel: String,
    pub deleted_at_ms: i64,
    pub is_dir: bool,
    pub size_bytes: u64,
}

/// `GET /peers/{peerId}/share/trash` response page.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareTrashPageDto {
    pub entries: Vec<PeerShareTrashEntryDto>,
    pub cursor: Option<String>,
    pub truncated: bool,
}

// ── Peer side: mutations ────────────────────────────────────────────────────

/// `POST …/share/mkdir` body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareMkdirRequest {
    pub root: String,
    /// Root-relative path of the directory to create.
    pub rel: String,
}

/// `POST …/share/rename` body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareRenameRequest {
    pub root: String,
    pub from: String,
    pub to: String,
}

/// `POST …/share/delete` body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareDeleteRequest {
    pub root: String,
    /// Root-relative path moved into the peer's trash.
    pub rel: String,
}

/// `POST …/share/copy` body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareCopyRequest {
    pub root: String,
    pub from: String,
    pub to: String,
}

/// `POST …/share/empty-trash` body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareEmptyTrashRequest {
    pub root: String,
}

/// `POST …/share/trash/restore` body. `to` is required on the wire (spec §9.1);
/// the frontend takes the default from `originalRel` after `ListTrash`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareRestoreRequest {
    pub root: String,
    pub trash_id: String,
    /// Root-relative restore target.
    pub to: String,
}

/// Stable result of every mutation verb.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    rename_all_fields = "camelCase"
)]
pub enum PeerShareMutationResultDto {
    /// mkdir / rename / copy / restore / empty-trash pure acknowledgement.
    Ack,
    /// delete result: the opaque id the entry got inside the peer's trash.
    Deleted { trash_id: String },
}

// ── Peer side: transfers ────────────────────────────────────────────────────

/// `POST …/share/upload` body. `sources` are local host file/directory paths;
/// the daemon registers them as `HostFileHandle`s so they never hit logs.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareUploadRequest {
    pub root: String,
    /// Root-relative destination directory (absent/empty = root top level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_dir: Option<String>,
    /// Local absolute paths of the files/directories to upload.
    pub sources: Vec<String>,
}

/// `POST …/share/download` body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareDownloadRequest {
    pub root: String,
    /// Root-relative path of the file to pull.
    pub rel: String,
    /// Local absolute target file path.
    pub target: String,
}

/// `202` response for upload/download: the caller-issued transfer id the
/// `remote-share.progress` / `remote-share.status_changed` WS events key off.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareTransferStartedDto {
    pub transfer_id: String,
}

/// `POST /remote-share/transfers/{id}/cancel` outcome.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerShareTransferCancelOutcomeDto {
    Cancelled,
    NotInflight,
    TooLate,
}

/// `POST /remote-share/transfers/{id}/cancel` response.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerShareTransferCancelResultDto {
    pub outcome: PeerShareTransferCancelOutcomeDto,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire field names are camelCase — pinned so the frontend SDK does
    /// not silently regress on a serde attribute edit.
    #[test]
    fn request_dtos_serialize_camel_case() {
        let req = PeerShareUploadRequest {
            root: "r".to_string(),
            dest_dir: None,
            sources: vec!["/tmp/a".to_string()],
        };
        let wire = serde_json::to_value(&req).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"root": "r", "sources": ["/tmp/a"]})
        );
    }

    #[test]
    fn list_response_is_kind_tagged() {
        let roots = PeerShareListResponse::Roots { roots: vec![] };
        assert_eq!(
            serde_json::to_value(&roots).unwrap(),
            serde_json::json!({"kind": "roots", "roots": []})
        );
        let page = PeerShareListResponse::Directory {
            entries: vec![],
            cursor: Some("a.txt".to_string()),
            truncated: true,
        };
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            serde_json::json!({
                "kind": "directory",
                "entries": [],
                "cursor": "a.txt",
                "truncated": true,
            })
        );
    }

    #[test]
    fn mutation_result_keeps_deleted_trash_id_camel_case() {
        let deleted = PeerShareMutationResultDto::Deleted {
            trash_id: "t-1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&deleted).unwrap(),
            serde_json::json!({"kind": "deleted", "trashId": "t-1"})
        );
    }
}
