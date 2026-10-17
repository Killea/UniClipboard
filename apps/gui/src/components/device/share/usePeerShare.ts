import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { DaemonApiError } from '@/api/daemon/errors'
import { remoteShareApi } from '@/api/daemon/remote-share'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import {
  navigatePeerShare,
  peerShareAvailability,
  peerShareListingError,
  peerShareListingReady,
  peerShareListingStarted,
  peerShareRootsLoaded,
  peerShareTransferStarted,
  peerShareTreeInvalidate,
  peerShareTreeLoaded,
  peerShareTreeLoading,
  remoteShareTreeKey,
  selectPeerShareRoot,
  openPeerShareTrash,
} from '@/store/slices/remoteShareSlice'

const log = createLogger('peer-share')

/** Stable `code` string of a daemon API error, when the body carried one. */
export function remoteShareErrorCode(err: unknown): string | null {
  if (err instanceof DaemonApiError) {
    const details = err.details as { code?: unknown } | undefined
    if (details && typeof details.code === 'string') return details.code
  }
  return null
}

/**
 * Browse + mutate driver for one peer's share surface. All remote tree/path
 * state lives in the `remoteShare` slice (in-memory only, spec §11.8) — this
 * hook only orchestrates fetches and dispatches results.
 */
export function usePeerShare(peerId: string) {
  const dispatch = useAppDispatch()
  const { t } = useTranslation()

  const refreshAvailability = useCallback(async () => {
    try {
      const { state } = await remoteShareApi.getPeerState(peerId)
      dispatch(peerShareAvailability({ peerId, availability: state }))
    } catch (err) {
      // A failed probe (unreachable/503) leaves availability null; the tab
      // hides itself for offline peers via the caller's `connected` check.
      log.warn({ err, peerId }, 'peer share state probe failed')
      dispatch(peerShareAvailability({ peerId, availability: null }))
    }
  }, [dispatch, peerId])

  const loadRoots = useCallback(async () => {
    try {
      const roots = await remoteShareApi.listRoots(peerId)
      dispatch(peerShareRootsLoaded({ peerId, roots }))
      return roots
    } catch (err) {
      log.warn({ err, peerId }, 'peer share root list failed')
      dispatch(peerShareRootsLoaded({ peerId, roots: [] }))
      return []
    }
  }, [dispatch, peerId])

  const loadListing = useCallback(
    async (rootId: string, dir: string, inTrash: boolean, append: boolean, cursor?: string) => {
      dispatch(peerShareListingStarted({ peerId, append }))
      try {
        if (inTrash) {
          const page = await remoteShareApi.listTrash(peerId, rootId, cursor)
          dispatch(
            peerShareListingReady({
              peerId,
              append,
              trashEntries: page.entries,
              cursor: page.cursor ?? null,
              truncated: page.truncated,
            })
          )
        } else {
          const page = await remoteShareApi.listPeerShare(peerId, {
            root: rootId,
            dir,
            cursor,
          })
          dispatch(
            peerShareListingReady({
              peerId,
              append,
              entries: page.kind === 'directory' ? page.entries : [],
              cursor: page.kind === 'directory' ? (page.cursor ?? null) : null,
              truncated: page.kind === 'directory' ? page.truncated : false,
            })
          )
        }
      } catch (err) {
        const code = remoteShareErrorCode(err) ?? 'io'
        log.warn({ err, peerId, rootId, dir, inTrash }, 'peer share listing failed')
        dispatch(peerShareListingError({ peerId, error: code }))
      }
    },
    [dispatch, peerId]
  )

  const loadTreeChildren = useCallback(
    async (rootId: string, rel: string) => {
      const key = remoteShareTreeKey(peerId, rootId, rel)
      dispatch(peerShareTreeLoading({ peerId, rootId, rel }))
      try {
        // Depth guard is a client constraint (spec §11.3) — the tree never
        // expands past it; the list keeps browsing deeper via pagination.
        const page = await remoteShareApi.listPeerShare(peerId, {
          root: rootId,
          dir: rel,
        })
        dispatch(
          peerShareTreeLoaded({
            peerId,
            rootId,
            rel,
            entries: page.kind === 'directory' ? page.entries : [],
          })
        )
      } catch (err) {
        log.warn({ err, peerId, key }, 'peer share tree load failed')
        dispatch(peerShareTreeLoaded({ peerId, rootId, rel, entries: [] }))
      }
    },
    [dispatch, peerId]
  )

  /** Run a mutation, toast on failure, then refresh the listing + tree cache. */
  const mutate = useCallback(
    async (
      refresh: { rootId: string; dir: string; inTrash: boolean },
      run: () => Promise<unknown>
    ): Promise<boolean> => {
      try {
        await run()
      } catch (err) {
        const code = remoteShareErrorCode(err)
        log.warn({ err, peerId }, 'peer share mutation failed')
        toast.error(
          t(`devices.share.errors.${code ?? 'io'}`, {
            defaultValue: t('devices.share.errors.io'),
          })
        )
        return false
      }
      dispatch(peerShareTreeInvalidate({ peerId, rootId: refresh.rootId }))
      void loadListing(refresh.rootId, refresh.dir, refresh.inTrash, false)
      return true
    },
    [dispatch, loadListing, peerId, t]
  )

  const upload = useCallback(
    async (
      rootId: string,
      dir: string,
      sources: string[],
      totalBytes: number | null,
      label: string
    ) => {
      try {
        const { transferId } = await remoteShareApi.upload(peerId, rootId, sources, dir)
        dispatch(
          peerShareTransferStarted({
            transferId,
            peerId,
            direction: 'upload',
            label,
            totalBytes,
          })
        )
      } catch (err) {
        const code = remoteShareErrorCode(err)
        log.warn({ err, peerId, rootId }, 'peer share upload rejected')
        toast.error(t(`devices.share.errors.${code ?? 'io'}`, { defaultValue: code ?? '' }))
      }
    },
    [dispatch, peerId, t]
  )

  const download = useCallback(
    async (rootId: string, rel: string, target: string, label: string) => {
      try {
        const { transferId } = await remoteShareApi.download(peerId, rootId, rel, target)
        dispatch(peerShareTransferStarted({ transferId, peerId, direction: 'download', label }))
      } catch (err) {
        const code = remoteShareErrorCode(err)
        log.warn({ err, peerId, rootId, rel }, 'peer share download rejected')
        toast.error(t(`devices.share.errors.${code ?? 'io'}`, { defaultValue: code ?? '' }))
      }
    },
    [dispatch, peerId, t]
  )

  return {
    refreshAvailability,
    loadRoots,
    loadListing,
    loadTreeChildren,
    mutate,
    upload,
    download,
    selectRoot: (rootId: string) => dispatch(selectPeerShareRoot({ peerId, rootId })),
    navigate: (dir: string) => dispatch(navigatePeerShare({ peerId, dir })),
    openTrash: () => dispatch(openPeerShareTrash({ peerId })),
  }
}
