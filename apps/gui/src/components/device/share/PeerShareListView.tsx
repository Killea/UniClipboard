import { File, Folder, MoreHorizontal, RotateCcw } from 'lucide-react'
import React, { useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import type { PeerShareEntryDto, PeerShareRootDto } from '@/api/daemon/remote-share'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/motion/context-menu'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '@/lib/utils'
import type { RemoteShareBrowse } from '@/store/slices/remoteShareSlice'
import { formatFileSize } from '@/utils/formatters'
import { relBaseName } from './peer-share-paths'

export type PeerShareSelection = Set<string>

interface PeerShareListViewProps {
  browse: RemoteShareBrowse | undefined
  inTrash: boolean
  root: PeerShareRootDto | undefined
  selection: PeerShareSelection
  onSelectionChange: (next: PeerShareSelection) => void
  onOpenDir: (name: string) => void
  onDownload: (entry: PeerShareEntryDto) => void
  onDownloadSelection: () => void
  onRename: (entry: PeerShareEntryDto) => void
  onCopy: (entry: PeerShareEntryDto) => void
  /** Directory mode: entry names → rels are joined by the caller. */
  onDelete: (keys: string[]) => void
  onRestore: (trashId: string, originalRel: string) => void
  onEmptyTrash: () => void
  onLoadMore: () => void
}

/**
 * Right-hand listing of the peer Files tab: checkbox multi-select, inline
 * `⋯` + right-click action menus, cursor pagination (spec §11.3). Trash rows
 * are driven by ListTrash and offer restore-only actions — permanent removal
 * exists solely as 「清空回收站」 (§11.7).
 */
const PeerShareListView: React.FC<PeerShareListViewProps> = ({
  browse,
  inTrash,
  root,
  selection,
  onSelectionChange,
  onOpenDir,
  onDownload,
  onDownloadSelection,
  onRename,
  onCopy,
  onDelete,
  onRestore,
  onEmptyTrash,
  onLoadMore,
}) => {
  const { t } = useTranslation()

  const toggleKey = useCallback(
    (key: string) => {
      const next = new Set(selection)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      onSelectionChange(next)
    },
    [selection, onSelectionChange]
  )

  if (
    browse?.phase === 'loading' &&
    browse.entries.length === 0 &&
    browse.trashEntries.length === 0
  ) {
    return (
      <div className="flex flex-1 items-center justify-center text-ui-caption text-muted-foreground">
        {t('devices.share.loading')}
      </div>
    )
  }
  if (browse?.phase === 'error') {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 text-muted-foreground">
        <p className="text-ui-body">
          {t(`devices.share.errors.${browse.error ?? 'io'}`, {
            defaultValue: t('devices.share.errors.io'),
          })}
        </p>
      </div>
    )
  }

  const entries = browse?.entries ?? []
  const trashEntries = browse?.trashEntries ?? []
  const rowCount = inTrash ? trashEntries.length : entries.length
  const selectionNames = inTrash
    ? trashEntries.filter(e => selection.has(e.trashId)).map(e => e.originalRel)
    : entries.filter(e => selection.has(e.name)).map(e => e.name)

  const entryMenu = (entry: PeerShareEntryDto) => (
    <ContextMenuContent ariaLabel={entry.name} className="w-48">
      {!entry.isDir && (
        <ContextMenuItem onSelect={() => onDownload(entry)} disabled={!root?.read}>
          {t('devices.share.actions.download')}
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={() => onRename(entry)} disabled={!root?.write}>
        {t('devices.share.actions.rename')}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => onCopy(entry)} disabled={!root?.write}>
        {t('devices.share.actions.copy')}
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem
        className="text-destructive"
        onSelect={() => onDelete([entry.name])}
        disabled={!root?.delete}
      >
        {t('devices.share.actions.delete')}
      </ContextMenuItem>
    </ContextMenuContent>
  )

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* selection action bar */}
      {selection.size > 0 && (
        <div className="flex items-center gap-2 border-b border-border/50 bg-muted/40 px-3 py-1.5 text-ui-caption">
          <span>{t('devices.share.selected', { count: selection.size })}</span>
          <span className="flex-1" />
          {!inTrash && (
            <Button variant="ghost" size="sm" onClick={onDownloadSelection}>
              {t('devices.share.actions.download')}
            </Button>
          )}
          {!inTrash && root?.delete && (
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive hover:text-destructive"
              onClick={() => onDelete(selectionNames)}
            >
              {t('devices.share.actions.delete')}
            </Button>
          )}
          <Button variant="ghost" size="sm" onClick={() => onSelectionChange(new Set())}>
            {t('devices.share.actions.clearSelection')}
          </Button>
        </div>
      )}
      {inTrash && (
        <div className="flex items-center justify-between border-b border-border/50 px-3 py-1.5">
          <span className="text-ui-caption text-muted-foreground">
            {t('devices.share.trashHint')}
          </span>
          <Button
            variant="ghost"
            size="sm"
            className="text-destructive hover:text-destructive"
            disabled={!root?.delete}
            onClick={onEmptyTrash}
          >
            {t('devices.share.actions.emptyTrash')}
          </Button>
        </div>
      )}

      <ScrollArea className="min-h-0 flex-1">
        {rowCount === 0 && (
          <p className="px-4 py-8 text-center text-ui-caption text-muted-foreground">
            {inTrash ? t('devices.share.trashEmpty') : t('devices.share.dirEmpty')}
          </p>
        )}
        <ul>
          {!inTrash &&
            entries.map(entry => (
              <li key={entry.name}>
                <ContextMenu>
                  <ContextMenuTrigger>
                    <div
                      className={cn(
                        'group flex items-center gap-2 px-3 py-1.5 hover:bg-foreground/5',
                        selection.has(entry.name) && 'bg-foreground/6'
                      )}
                    >
                      <Checkbox
                        checked={selection.has(entry.name)}
                        onCheckedChange={() => toggleKey(entry.name)}
                        aria-label={entry.name}
                        className="size-3.5"
                      />
                      {entry.isDir ? (
                        <Folder className="size-4 shrink-0 text-muted-foreground" />
                      ) : (
                        <File className="size-4 shrink-0 text-muted-foreground" />
                      )}
                      <button
                        type="button"
                        className="min-w-0 flex-1 truncate text-left text-ui-body"
                        onClick={() => entry.isDir && onOpenDir(entry.name)}
                        title={entry.name}
                      >
                        {entry.name}
                      </button>
                      <span className="w-20 shrink-0 text-right text-ui-caption text-muted-foreground tabular-nums">
                        {entry.isDir ? '—' : formatFileSize(entry.sizeBytes)}
                      </span>
                      <ContextMenu>
                        <ContextMenuTrigger activation="click">
                          <button
                            type="button"
                            aria-label={t('devices.share.actions.menu')}
                            className="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground opacity-0 hover:bg-muted group-hover:opacity-100"
                          >
                            <MoreHorizontal className="size-4" />
                          </button>
                        </ContextMenuTrigger>
                        {entryMenu(entry)}
                      </ContextMenu>
                    </div>
                  </ContextMenuTrigger>
                  {entryMenu(entry)}
                </ContextMenu>
              </li>
            ))}
          {inTrash &&
            trashEntries.map(entry => (
              <li key={entry.trashId}>
                <ContextMenu>
                  <ContextMenuTrigger>
                    <div
                      className={cn(
                        'group flex items-center gap-2 px-3 py-1.5 hover:bg-foreground/5',
                        selection.has(entry.trashId) && 'bg-foreground/6'
                      )}
                    >
                      <Checkbox
                        checked={selection.has(entry.trashId)}
                        onCheckedChange={() => toggleKey(entry.trashId)}
                        aria-label={entry.originalRel}
                        className="size-3.5"
                      />
                      {entry.isDir ? (
                        <Folder className="size-4 shrink-0 text-muted-foreground" />
                      ) : (
                        <File className="size-4 shrink-0 text-muted-foreground" />
                      )}
                      <span
                        className="min-w-0 flex-1 truncate text-left text-ui-body"
                        title={entry.originalRel}
                      >
                        {relBaseName(entry.originalRel)}
                      </span>
                      <span className="w-20 shrink-0 text-right text-ui-caption text-muted-foreground tabular-nums">
                        {entry.isDir ? '—' : formatFileSize(entry.sizeBytes)}
                      </span>
                      <ContextMenu>
                        <ContextMenuTrigger activation="click">
                          <button
                            type="button"
                            aria-label={t('devices.share.actions.menu')}
                            className="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground opacity-0 hover:bg-muted group-hover:opacity-100"
                          >
                            <MoreHorizontal className="size-4" />
                          </button>
                        </ContextMenuTrigger>
                        <ContextMenuContent ariaLabel={entry.originalRel} className="w-48">
                          <ContextMenuItem
                            onSelect={() => onRestore(entry.trashId, entry.originalRel)}
                            disabled={!root?.write}
                          >
                            <RotateCcw className="size-3.5" />
                            {t('devices.share.actions.restore')}
                          </ContextMenuItem>
                        </ContextMenuContent>
                      </ContextMenu>
                    </div>
                  </ContextMenuTrigger>
                  <ContextMenuContent ariaLabel={entry.originalRel} className="w-48">
                    <ContextMenuItem
                      onSelect={() => onRestore(entry.trashId, entry.originalRel)}
                      disabled={!root?.write}
                    >
                      {t('devices.share.actions.restore')}
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
              </li>
            ))}
        </ul>
        {browse?.cursor && (
          <div className="flex justify-center py-2">
            <Button variant="outline" size="sm" onClick={onLoadMore}>
              {t('devices.share.loadMore')}
            </Button>
          </div>
        )}
      </ScrollArea>
    </div>
  )
}

export default PeerShareListView
