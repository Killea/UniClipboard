import { ChevronRight, Folder, FolderOpen, HardDrive, Trash2 } from 'lucide-react'
import React, { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { PeerShareEntryDto, PeerShareRootDto } from '@/api/daemon/remote-share'
import { PEER_SHARE_MAX_DEPTH } from '@/api/daemon/remote-share'
import { cn } from '@/lib/utils'
import { remoteShareTreeKey } from '@/store/slices/remoteShareSlice'
import { relJoin } from './peer-share-paths'

interface PeerShareTreeProps {
  peerId: string
  roots: PeerShareRootDto[]
  selectedRootId: string | null
  /** Current root-relative dir of the file list. */
  dir: string
  inTrash: boolean
  treeChildren: Record<string, PeerShareEntryDto[]>
  treeLoading: Record<string, boolean>
  onNavigate: (rootId: string, dir: string) => void
  onOpenTrash: () => void
  /** Fetch children for `rootId:rel` — called once per expand. */
  onExpand: (rootId: string, rel: string) => void
}

/**
 * Lazy directory tree (spec §11.3): roots at level 0, directories expand on
 * demand, and each root carries a virtual 「回收站」 node that is not a real
 * path (§11.7). Expansion stops at the client depth cap.
 */
const PeerShareTree: React.FC<PeerShareTreeProps> = ({
  peerId,
  roots,
  selectedRootId,
  dir,
  inTrash,
  treeChildren,
  treeLoading,
  onNavigate,
  onOpenTrash,
  onExpand,
}) => {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState<Set<string>>(new Set())

  const toggle = useCallback(
    (rootId: string, rel: string) => {
      const key = remoteShareTreeKey(peerId, rootId, rel)
      setExpanded(prev => {
        const next = new Set(prev)
        if (next.has(key)) next.delete(key)
        else {
          next.add(key)
          if (treeChildren[key] === undefined) onExpand(rootId, rel)
        }
        return next
      })
    },
    [peerId, treeChildren, onExpand]
  )

  const renderDir = (rootId: string, rel: string, name: string, depth: number) => {
    const key = remoteShareTreeKey(peerId, rootId, rel)
    const isExpanded = expanded.has(key)
    const children = treeChildren[key]
    const loading = treeLoading[key] === true
    const isActive = rootId === selectedRootId && !inTrash && dir === rel
    const canExpand = depth < PEER_SHARE_MAX_DEPTH

    return (
      <div key={key}>
        <button
          type="button"
          style={{ paddingLeft: `${depth * 12 + 8}px` }}
          className={cn(
            'flex w-full items-center gap-1 rounded-md px-2 py-1 text-left text-ui-caption hover:bg-foreground/5',
            isActive && 'bg-foreground/8 text-foreground'
          )}
          onClick={() => {
            if (canExpand) toggle(rootId, rel)
            onNavigate(rootId, rel)
          }}
        >
          <ChevronRight
            className={cn(
              'size-3 shrink-0 text-muted-foreground transition-transform',
              isExpanded && 'rotate-90',
              !canExpand && 'invisible'
            )}
          />
          {isExpanded ? (
            <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
          ) : (
            <Folder className="size-3.5 shrink-0 text-muted-foreground" />
          )}
          <span className="truncate">{name}</span>
        </button>
        {isExpanded && canExpand && (
          <div>
            {loading && (
              <p
                style={{ paddingLeft: `${(depth + 1) * 12 + 20}px` }}
                className="py-1 text-ui-caption text-muted-foreground/60"
              >
                …
              </p>
            )}
            {children?.map(child =>
              renderDir(rootId, relJoin(rel, child.name), child.name, depth + 1)
            )}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-0.5 p-1.5">
      {roots.map(r => {
        const rootKey = remoteShareTreeKey(peerId, r.id, '')
        const isExpanded = expanded.has(rootKey)
        const children = treeChildren[rootKey]
        const loading = treeLoading[rootKey] === true
        const rootActive = r.id === selectedRootId && !inTrash && dir === ''
        const trashActive = r.id === selectedRootId && inTrash
        return (
          <div key={r.id}>
            <button
              type="button"
              className={cn(
                'flex w-full items-center gap-1 rounded-md px-2 py-1 text-left text-ui-caption font-medium hover:bg-foreground/5',
                r.id === selectedRootId && !inTrash && 'text-foreground',
                rootActive && 'bg-foreground/8'
              )}
              onClick={() => {
                toggle(r.id, '')
                onNavigate(r.id, '')
              }}
            >
              <ChevronRight
                className={cn(
                  'size-3 shrink-0 text-muted-foreground transition-transform',
                  isExpanded && 'rotate-90'
                )}
              />
              <HardDrive className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="truncate">{r.label}</span>
            </button>
            {isExpanded && (
              <div>
                {loading && <p className="py-1 pl-8 text-ui-caption text-muted-foreground/60">…</p>}
                {children?.map(child => renderDir(r.id, child.name, child.name, 1))}
                {/* Virtual trash node — not a real path (§11.7). */}
                <button
                  type="button"
                  className={cn(
                    'flex w-full items-center gap-1 rounded-md px-2 py-1 pl-8 text-left text-ui-caption text-muted-foreground hover:bg-foreground/5 hover:text-foreground',
                    trashActive && 'bg-foreground/8 text-foreground'
                  )}
                  onClick={() => {
                    onNavigate(r.id, '')
                    onOpenTrash()
                  }}
                >
                  <Trash2 className="size-3.5 shrink-0" />
                  {t('devices.share.trash')}
                </button>
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}

export default PeerShareTree
