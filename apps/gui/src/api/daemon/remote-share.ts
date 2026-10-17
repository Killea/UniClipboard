/**
 * Daemon remote-share API module — typed wrappers for the session-scoped
 * cross-device file-share endpoints (`/remote-share/*` and
 * `/peers/{peer_id}/share/*`).
 *
 * Two surfaces:
 * - Local state: own share-root whitelist + suspend flag (settings page).
 * - Peer surface: availability, listing, stat, mutations, trash, and the
 *   202-async upload/download endpoints whose lifecycle is reported through
 *   `remote-share.*` events on the `file-transfer` WS topic.
 */

import {
  getPeerShareState as getPeerShareStateSdk,
  listPeerShare as listPeerShareSdk,
  statPeerShareEntry as statPeerShareEntrySdk,
  listPeerShareTrash as listPeerShareTrashSdk,
  mkdirPeerShare as mkdirPeerShareSdk,
  renamePeerShare as renamePeerShareSdk,
  copyPeerShareEntry as copyPeerShareEntrySdk,
  deletePeerShareEntry as deletePeerShareEntrySdk,
  restorePeerShareTrash as restorePeerShareTrashSdk,
  emptyPeerShareTrash as emptyPeerShareTrashSdk,
  uploadPeerShare as uploadPeerShareSdk,
  downloadPeerShare as downloadPeerShareSdk,
  getRemoteShareState as getRemoteShareStateSdk,
  replaceRemoteShareRoots as replaceRemoteShareRootsSdk,
  setRemoteShareSuspended as setRemoteShareSuspendedSdk,
  cancelRemoteShareTransfer as cancelRemoteShareTransferSdk,
} from '@/api/generated/sdk.gen'
import type {
  PeerShareAvailability,
  PeerShareEntryDto,
  PeerShareListResponse,
  PeerShareMutationResultDto,
  PeerShareTrashPageDto,
  PeerShareTransferCancelResultDto,
  RemoteShareReconcileDto,
  RemoteShareRootInputDto,
  RemoteShareStateDto,
} from '@/api/generated/types.gen'
import { daemonClient } from './client'

export type {
  PeerShareAvailability,
  PeerShareEntryDto,
  PeerShareMutationResultDto,
  PeerShareRootDto,
  PeerShareTrashEntryDto,
  PeerShareTrashPageDto,
  RemoteShareRootDto,
  RemoteShareRootInputDto,
  RemoteShareStateDto,
} from '@/api/generated/types.gen'

/** Client-side listing caps (spec §11.3): the server truncates at its own
 * bound; the UI never asks for more than this. */
export const PEER_SHARE_PAGE_LIMIT = 2000
export const PEER_SHARE_MAX_DEPTH = 8

export const remoteShareApi = {
  // ── local share state ────────────────────────────────────────
  getLocalState: () =>
    daemonClient.callEnveloped<RemoteShareStateDto>(() =>
      getRemoteShareStateSdk({ throwOnError: true })
    ),

  replaceRoots: (roots: RemoteShareRootInputDto[]) =>
    daemonClient.callEnveloped<RemoteShareReconcileDto>(() =>
      replaceRemoteShareRootsSdk({ body: { roots }, throwOnError: true })
    ),

  setSuspended: (suspended: boolean) =>
    daemonClient.callEnveloped<RemoteShareReconcileDto>(() =>
      setRemoteShareSuspendedSdk({ body: { suspended }, throwOnError: true })
    ),

  // ── peer surface ─────────────────────────────────────────────
  getPeerState: (peerId: string) =>
    daemonClient.callEnveloped<{ state: PeerShareAvailability }>(() =>
      getPeerShareStateSdk({ path: { peer_id: peerId }, throwOnError: true })
    ),

  /** `root` absent → root list; `dir` absent → root top level. */
  listPeerShare: (
    peerId: string,
    query: { root?: string; dir?: string; cursor?: string; limit?: number }
  ) =>
    daemonClient.callEnveloped<PeerShareListResponse>(() =>
      listPeerShareSdk({
        path: { peer_id: peerId },
        query: {
          root: query.root ?? null,
          dir: query.dir ?? null,
          cursor: query.cursor ?? null,
          limit: query.limit ?? PEER_SHARE_PAGE_LIMIT,
        },
        throwOnError: true,
      })
    ),

  listRoots: async (peerId: string) => {
    const page = await remoteShareApi.listPeerShare(peerId, {})
    return page.kind === 'roots' ? page.roots : []
  },

  statEntry: (peerId: string, root: string, rel: string) =>
    daemonClient.callEnveloped<PeerShareEntryDto>(() =>
      statPeerShareEntrySdk({
        path: { peer_id: peerId },
        query: { root, rel },
        throwOnError: true,
      })
    ),

  listTrash: (peerId: string, root: string, cursor?: string) =>
    daemonClient.callEnveloped<PeerShareTrashPageDto>(() =>
      listPeerShareTrashSdk({
        path: { peer_id: peerId },
        query: { root, cursor: cursor ?? null, limit: PEER_SHARE_PAGE_LIMIT },
        throwOnError: true,
      })
    ),

  mkdir: (peerId: string, root: string, rel: string) =>
    daemonClient.callEnveloped<PeerShareMutationResultDto>(() =>
      mkdirPeerShareSdk({ path: { peer_id: peerId }, body: { root, rel }, throwOnError: true })
    ),

  rename: (peerId: string, root: string, from: string, to: string) =>
    daemonClient.callEnveloped<PeerShareMutationResultDto>(() =>
      renamePeerShareSdk({
        path: { peer_id: peerId },
        body: { root, from, to },
        throwOnError: true,
      })
    ),

  copy: (peerId: string, root: string, from: string, to: string) =>
    daemonClient.callEnveloped<PeerShareMutationResultDto>(() =>
      copyPeerShareEntrySdk({
        path: { peer_id: peerId },
        body: { root, from, to },
        throwOnError: true,
      })
    ),

  deleteEntry: (peerId: string, root: string, rel: string) =>
    daemonClient.callEnveloped<PeerShareMutationResultDto>(() =>
      deletePeerShareEntrySdk({
        path: { peer_id: peerId },
        body: { root, rel },
        throwOnError: true,
      })
    ),

  restoreTrash: (peerId: string, root: string, trashId: string, to: string) =>
    daemonClient.callEnveloped<PeerShareMutationResultDto>(() =>
      restorePeerShareTrashSdk({
        path: { peer_id: peerId },
        body: { root, trashId, to },
        throwOnError: true,
      })
    ),

  emptyTrash: (peerId: string, root: string) =>
    daemonClient.callEnveloped<PeerShareMutationResultDto>(() =>
      emptyPeerShareTrashSdk({ path: { peer_id: peerId }, body: { root }, throwOnError: true })
    ),

  /** `202` — the returned transfer id keys `remote-share.*` WS events. */
  upload: (peerId: string, root: string, sources: string[], destDir?: string) =>
    daemonClient.callEnveloped<{ transferId: string }>(() =>
      uploadPeerShareSdk({
        path: { peer_id: peerId },
        body: { root, sources, destDir: destDir ?? null },
        throwOnError: true,
      })
    ),

  download: (peerId: string, root: string, rel: string, target: string) =>
    daemonClient.callEnveloped<{ transferId: string }>(() =>
      downloadPeerShareSdk({
        path: { peer_id: peerId },
        body: { root, rel, target },
        throwOnError: true,
      })
    ),

  cancelTransfer: (transferId: string) =>
    daemonClient.callEnveloped<PeerShareTransferCancelResultDto>(() =>
      cancelRemoteShareTransferSdk({ path: { transfer_id: transferId }, throwOnError: true })
    ),
}
