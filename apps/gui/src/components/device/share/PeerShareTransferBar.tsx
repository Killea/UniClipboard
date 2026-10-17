import { ChevronUp, X } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { remoteShareApi } from '@/api/daemon/remote-share'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  peerShareTransferDismissed,
  selectRemoteShareTransfers,
  type RemoteShareTransfer,
} from '@/store/slices/remoteShareSlice'
import { formatFileSize } from '@/utils/formatters'

const log = createLogger('peer-share-transfers')

/**
 * Bottom transfer strip of the peer Files tab (spec §11.3): one-line summary
 * of in-flight remote-share transfers for this peer, expandable to the queue
 * with cancel / dismiss controls. Progress comes from `remote-share.*` WS
 * events — no polling.
 */
const PeerShareTransferBar: React.FC<{ peerId: string }> = ({ peerId }) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const [expanded, setExpanded] = useState(false)
  const transfers = useAppSelector(selectRemoteShareTransfers).filter(t_ => t_.peerId === peerId)

  if (transfers.length === 0) return null

  const active = transfers.filter(t_ => t_.status === 'active')
  const done = transfers.filter(t_ => t_.status !== 'active')

  const cancel = (id: string) => {
    void remoteShareApi.cancelTransfer(id).catch(err => {
      log.warn({ err, id }, 'remote-share cancel failed')
    })
  }

  const row = (t_: RemoteShareTransfer) => {
    const pct =
      t_.totalBytes && t_.totalBytes > 0
        ? Math.min(100, Math.round((t_.bytesTransferred / t_.totalBytes) * 100))
        : null
    return (
      <li key={t_.transferId} className="flex items-center gap-2 px-3 py-1">
        <span className="min-w-0 flex-1 truncate text-ui-caption">{t_.label}</span>
        <span className="w-14 shrink-0 text-ui-caption text-muted-foreground">
          {t_.direction === 'upload' ? '↑' : '↓'}
        </span>
        {t_.status === 'active' ? (
          <>
            <Progress value={pct ?? 0} className="h-1 w-24 shrink-0" />
            <span className="w-14 shrink-0 text-right text-ui-caption text-muted-foreground tabular-nums">
              {pct !== null ? `${pct}%` : '…'}
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="size-6 shrink-0"
              aria-label={t('devices.share.actions.cancelTransfer')}
              onClick={() => cancel(t_.transferId)}
            >
              <X className="size-3" />
            </Button>
          </>
        ) : (
          <>
            <span
              className={cn(
                'w-24 shrink-0 text-ui-caption',
                t_.status === 'failed' ? 'text-destructive' : 'text-muted-foreground'
              )}
            >
              {t(`devices.share.transfer.${t_.status}`)}
              {t_.status === 'failed' && t_.reason ? ` · ${t_.reason}` : ''}
            </span>
            <span className="w-14 shrink-0 text-right text-ui-caption text-muted-foreground tabular-nums">
              {t_.totalBytes != null ? formatFileSize(t_.totalBytes) : ''}
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="size-6 shrink-0"
              aria-label={t('devices.share.actions.dismissTransfer')}
              onClick={() => dispatch(peerShareTransferDismissed({ transferId: t_.transferId }))}
            >
              <X className="size-3" />
            </Button>
          </>
        )}
      </li>
    )
  }

  return (
    <div className="shrink-0 rounded-lg border border-border/60 bg-card">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2 text-ui-caption hover:bg-foreground/5"
        onClick={() => setExpanded(e => !e)}
      >
        <ChevronUp className={cn('size-3.5 transition-transform', !expanded && 'rotate-180')} />
        <span className="flex-1 text-left">
          {active.length > 0
            ? t('devices.share.transfersActive', { count: active.length })
            : t('devices.share.transfersDone', { count: done.length })}
        </span>
      </button>
      {expanded && (
        <ul className="max-h-40 overflow-y-auto border-t border-border/50 pb-1">
          {transfers.map(row)}
        </ul>
      )}
    </div>
  )
}

export default PeerShareTransferBar
