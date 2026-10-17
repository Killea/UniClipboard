import { useEffect } from 'react'
import type { PeerShareAvailability } from '@/api/daemon/remote-share'
import { daemonWs } from '@/lib/daemon-ws'
import { useAppDispatch } from '@/store/hooks'
import {
  peerShareAvailability,
  peerShareTransferFinished,
  peerShareTransferProgress,
} from '@/store/slices/remoteShareSlice'

/**
 * Bridges the daemon's `remote-share.*` events (retagged `file-transfer`
 * topic events) into the remoteShare slice. Mounted by the peer Files tab;
 * daemon-side dedup already collapses duplicate terminals.
 */
export function useRemoteShareEvents() {
  const dispatch = useAppDispatch()

  useEffect(() => {
    const unsubscribe = daemonWs.subscribe(['file-transfer'], event => {
      const payload = event.payload as Record<string, unknown>
      switch (event.eventType) {
        case 'remote-share.progress':
          dispatch(
            peerShareTransferProgress({
              transferId: payload.transferId as string,
              peerId: payload.peerId as string,
              bytesTransferred: payload.bytesTransferred as number,
              totalBytes: (payload.totalBytes as number | null) ?? null,
            })
          )
          break
        case 'remote-share.status_changed':
          dispatch(
            peerShareTransferFinished({
              transferId: payload.transferId as string,
              status: payload.status as string,
              reason: (payload.reason as string | null) ?? null,
            })
          )
          break
        case 'remote-share.availability_changed':
          dispatch(
            peerShareAvailability({
              peerId: payload.peerId as string,
              availability: payload.state as PeerShareAvailability,
            })
          )
          break
      }
    })
    return unsubscribe
  }, [dispatch])
}
