import { listen } from '@tauri-apps/api/event'
import { FolderPlus, HardDrive, Lock, PauseCircle, RefreshCw, Upload } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { PEER_SHARE_MAX_DEPTH, remoteShareApi } from '@/api/daemon/remote-share'
import type { PeerShareEntryDto, PeerShareRootDto } from '@/api/daemon/remote-share'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { toast } from '@/components/ui/toast'
import { commands } from '@/lib/ipc'
import type { LocalPathStat } from '@/lib/ipc-bindings.generated'
import { createLogger } from '@/lib/logger'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { peerShareViewClosed, selectRemoteShareBrowse } from '@/store/slices/remoteShareSlice'
import { relDirName, relDepth, relJoin, relSegments } from './peer-share-paths'
import PeerShareDialogs, { type PendingUpload } from './PeerShareDialogs'
import PeerShareListView from './PeerShareListView'
import PeerShareTransferBar from './PeerShareTransferBar'
import PeerShareTree from './PeerShareTree'
import { usePeerShare } from './usePeerShare'
import { useRemoteShareEvents } from './useRemoteShareEvents'

const log = createLogger('peer-share-tab')

/** Drop zones declare themselves to the OS-level drop hook with this id. */
export const PEER_SHARE_DROP_TARGET = 'peer-share-drop'

interface PeerShareTabProps {
  peerId: string
  deviceName: string
  connected: boolean
}

/**
 * 「文件」tab of the peer detail panel (spec §11.3): root selector +
 * breadcrumbs + toolbar over a lazy tree / list split, a marked drop zone,
 * and the transfer queue bar. All remote state is Redux-in-memory only.
 */
const PeerShareTab: React.FC<PeerShareTabProps> = ({ peerId, deviceName, connected }) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const share = usePeerShare(peerId)
  const browse = useAppSelector(state => selectRemoteShareBrowse(state, peerId))
  const treeChildren = useAppSelector(state => state.remoteShare.treeChildren)
  const treeLoading = useAppSelector(state => state.remoteShare.treeLoading)

  const [selection, setSelection] = useState<Set<string>>(new Set())
  const [pendingUpload, setPendingUpload] = useState<PendingUpload | null>(null)
  const [nameDialog, setNameDialog] = useState<
    | { kind: 'mkdir' }
    | { kind: 'rename'; entry: PeerShareEntryDto }
    | { kind: 'copy'; entry: PeerShareEntryDto }
    | null
  >(null)
  const [emptyTrashOpen, setEmptyTrashOpen] = useState(false)

  useRemoteShareEvents()

  useEffect(() => {
    void share.refreshAvailability()
  }, [share.refreshAvailability])

  const availability = connected ? (browse?.availability ?? null) : null

  useEffect(() => {
    if (availability === 'ok' && browse && browse.roots === null) void share.loadRoots()
  }, [availability, browse, share.loadRoots])

  const rootId = browse?.selectedRootId ?? null
  const dir = browse?.dir ?? ''
  const inTrash = browse?.inTrash ?? false
  const phase = browse?.phase ?? 'idle'

  useEffect(() => {
    if (availability !== 'ok' || !rootId || phase !== 'idle') return
    void share.loadListing(rootId, dir, inTrash, false)
  }, [availability, rootId, dir, inTrash, phase, share.loadListing])

  // Leaving the peer frees its browse view + tree cache (in-memory only).
  useEffect(() => {
    return () => {
      dispatch(peerShareViewClosed({ peerId }))
    }
  }, [dispatch, peerId])

  const root: PeerShareRootDto | undefined = browse?.roots?.find(r => r.id === rootId)

  const refresh = useCallback(() => {
    if (!rootId) return
    void share.loadListing(rootId, dir, inTrash, false)
    void share.loadTreeChildren(rootId, relDirName(dir))
  }, [share, rootId, dir, inTrash])

  // ── uploads ───────────────────────────────────────────────────

  const stageUpload = useCallback(
    async (paths: string[]) => {
      if (!root || paths.length === 0) return
      if (!root.write) {
        toast.error(t('devices.share.errors.denied'))
        return
      }
      let stats: LocalPathStat[] = []
      try {
        stats = (await commands.statLocalPaths(paths)) ?? []
      } catch (err) {
        log.warn({ err }, 'stat_local_paths failed; pricing skipped')
      }
      const found = stats.filter(s => s.exists)
      setPendingUpload({
        paths,
        itemCount: found.length || paths.length,
        totalBytes: found.reduce((sum, s) => sum + s.sizeBytes, 0),
        peerName: deviceName,
        destination: `${root.label}:${dir === '' ? '/' : dir}`,
      })
    },
    [root, dir, deviceName, t]
  )

  useEffect(() => {
    const unlisten = listen<{ paths?: string[]; targetId?: string }>('files-dropped', event => {
      // Only drops on the marked zone start an upload (spec §11.6).
      if (event.payload.targetId !== PEER_SHARE_DROP_TARGET) return
      void stageUpload(event.payload.paths ?? [])
    })
    return () => {
      void unlisten.then(fn => fn())
    }
  }, [stageUpload])

  const confirmUpload = useCallback(() => {
    if (!pendingUpload || !root) return
    void share.upload(root.id, dir, pendingUpload.paths, pendingUpload.totalBytes, deviceName)
    setPendingUpload(null)
  }, [pendingUpload, root, dir, deviceName, share])

  const pickFiles = useCallback(() => {
    void commands
      .pickFiles()
      .then(paths => {
        if (paths?.length) void stageUpload(paths)
      })
      .catch(err => log.warn({ err }, 'pick_files failed'))
  }, [stageUpload])

  const pickFolder = useCallback(() => {
    void commands
      .pickDirectory()
      .then(path => {
        if (path) void stageUpload([path])
      })
      .catch(err => log.warn({ err }, 'pick_directory failed'))
  }, [stageUpload])

  // ── mutations ─────────────────────────────────────────────────

  const current = { rootId: rootId ?? '', dir, inTrash }

  const downloadEntry = useCallback(
    (entry: PeerShareEntryDto) => {
      if (!root || entry.isDir) return
      void commands
        .pickSavePath(entry.name)
        .then(target => {
          if (target) void share.download(root.id, relJoin(dir, entry.name), target, entry.name)
        })
        .catch(err => log.warn({ err }, 'pick_save_path failed'))
    },
    [root, dir, share]
  )

  const downloadSelection = useCallback(async () => {
    if (!root) return
    const picked = (browse?.entries ?? []).filter(e => selection.has(e.name) && !e.isDir)
    for (const entry of picked) {
      try {
        const target = await commands.pickSavePath(entry.name)
        if (target) void share.download(root.id, relJoin(dir, entry.name), target, entry.name)
      } catch (err) {
        log.warn({ err }, 'pick_save_path failed')
      }
    }
    setSelection(new Set())
  }, [root, browse?.entries, selection, dir, share])

  const deleteRels = useCallback(
    (rels: string[]) => {
      if (!root) return
      void share
        .mutate(current, async () => {
          for (const rel of rels) {
            await remoteShareApi.deleteEntry(peerId, root.id, rel)
          }
        })
        .then(ok => {
          if (ok) setSelection(new Set())
        })
    },
    [root, share, peerId, current]
  )

  const submitNameDialog = useCallback(
    (value: string) => {
      if (!root || !nameDialog) return
      const rel = relJoin(dir, value)
      if (nameDialog.kind === 'mkdir') {
        void share.mutate(current, () => remoteShareApi.mkdir(peerId, root.id, rel))
      } else if (nameDialog.kind === 'rename') {
        void share.mutate(current, () =>
          remoteShareApi.rename(peerId, root.id, relJoin(dir, nameDialog.entry.name), rel)
        )
      } else {
        void share.mutate(current, () =>
          remoteShareApi.copy(
            peerId,
            root.id,
            relJoin(dir, nameDialog.entry.name),
            relJoin(value, nameDialog.entry.name)
          )
        )
      }
      setNameDialog(null)
    },
    [nameDialog, root, dir, share, peerId, current]
  )

  const restoreTrashEntry = useCallback(
    (trashId: string, originalRel: string) => {
      if (!root) return
      void share.mutate(current, () =>
        remoteShareApi.restoreTrash(peerId, root.id, trashId, originalRel)
      )
    },
    [root, share, peerId, current]
  )

  const emptyTrash = useCallback(() => {
    if (!root) return
    void share.mutate(current, () => remoteShareApi.emptyTrash(peerId, root.id))
    setEmptyTrashOpen(false)
  }, [root, share, peerId, current])

  const loadMore = useCallback(() => {
    if (!rootId || !browse?.cursor) return
    void share.loadListing(rootId, dir, inTrash, true, browse.cursor)
  }, [rootId, dir, inTrash, browse?.cursor, share])

  // ── render states ─────────────────────────────────────────────

  if (!connected || availability === null) {
    return (
      <div className="flex h-40 items-center justify-center text-ui-caption text-muted-foreground">
        {connected ? t('devices.share.loading') : t('devices.share.offline')}
      </div>
    )
  }
  if (availability === 'locked') {
    return (
      <div className="flex h-40 flex-col items-center justify-center gap-3 text-muted-foreground">
        <Lock className="size-6" strokeWidth={1.5} />
        <p className="text-ui-body">{t('devices.share.locked')}</p>
        <Button variant="outline" size="sm" onClick={() => void share.refreshAvailability()}>
          {t('devices.share.retry')}
        </Button>
      </div>
    )
  }
  if (availability === 'suspended') {
    return (
      <div className="flex h-40 flex-col items-center justify-center gap-3 text-muted-foreground">
        <PauseCircle className="size-6" strokeWidth={1.5} />
        <p className="text-ui-body">{t('devices.share.suspended')}</p>
      </div>
    )
  }
  if (availability !== 'ok') return null

  const roots = browse?.roots ?? null
  if (roots === null) {
    return (
      <div className="flex h-40 items-center justify-center text-ui-caption text-muted-foreground">
        {t('devices.share.loading')}
      </div>
    )
  }
  if (roots.length === 0) {
    // No link to local settings here — the peer configures its own roots on
    // that device (spec §11.4).
    return (
      <div className="flex h-40 flex-col items-center justify-center gap-3 px-8 text-center text-muted-foreground">
        <HardDrive className="size-6" strokeWidth={1.5} />
        <p className="text-ui-body">{t('devices.share.noRoots')}</p>
        <p className="text-ui-caption">{t('devices.share.noRootsHint')}</p>
      </div>
    )
  }

  const depthLimitHit = !inTrash && relDepth(dir) >= PEER_SHARE_MAX_DEPTH

  return (
    // The tab lives inside the page ScrollArea, so the browser split gets a
    // viewport-bounded height for its inner tree/list scroll areas.
    <div className="flex h-[calc(100dvh-16rem)] min-h-96 flex-col gap-3">
      {/* ── root selector + breadcrumbs + toolbar ─────────────── */}
      <div className="flex flex-wrap items-center gap-2">
        <Select value={rootId ?? undefined} onValueChange={v => share.selectRoot(v)}>
          <SelectTrigger size="sm" className="w-44" aria-label={t('devices.share.rootPicker')}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {roots.map(r => (
              <SelectItem key={r.id} value={r.id}>
                {r.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <nav className="flex min-w-0 flex-1 items-center gap-1 text-ui-caption text-muted-foreground">
          <button
            type="button"
            className="hover:text-foreground"
            onClick={() => share.navigate('')}
          >
            {root?.label}
          </button>
          {inTrash ? (
            <>
              <span>/</span>
              <span className="text-foreground">{t('devices.share.trash')}</span>
            </>
          ) : (
            relSegments(dir).map((seg, i) => {
              const target = relSegments(dir)
                .slice(0, i + 1)
                .join('/')
              return (
                <React.Fragment key={target}>
                  <span>/</span>
                  <button
                    type="button"
                    className="hover:text-foreground"
                    onClick={() => share.navigate(target)}
                  >
                    {seg}
                  </button>
                </React.Fragment>
              )
            })
          )}
        </nav>

        <div className="flex items-center gap-1.5">
          <Button
            variant="outline"
            size="sm"
            onClick={pickFiles}
            disabled={!root?.write || inTrash}
          >
            <Upload className="size-3.5" />
            {t('devices.share.uploadFiles')}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={pickFolder}
            disabled={!root?.write || inTrash}
          >
            <Upload className="size-3.5" />
            {t('devices.share.uploadFolder')}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setNameDialog({ kind: 'mkdir' })}
            disabled={!root?.write || inTrash}
          >
            <FolderPlus className="size-3.5" />
            {t('devices.share.mkdir')}
          </Button>
          <Button
            variant="outline"
            size="icon"
            className="size-8"
            onClick={refresh}
            aria-label={t('devices.share.refresh')}
          >
            <RefreshCw className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* ── tree + list split ─────────────────────────────────── */}
      <div className="flex min-h-0 flex-1 gap-3">
        <ScrollArea className="w-44 shrink-0 rounded-lg border border-border/60 bg-card/50">
          <PeerShareTree
            peerId={peerId}
            roots={roots}
            selectedRootId={rootId}
            dir={dir}
            inTrash={inTrash}
            treeChildren={treeChildren}
            treeLoading={treeLoading}
            onNavigate={(nextRoot, nextDir) => {
              if (nextRoot !== rootId) share.selectRoot(nextRoot)
              if (nextDir !== dir || nextRoot !== rootId) share.navigate(nextDir)
            }}
            onOpenTrash={share.openTrash}
            onExpand={(rId, rel) => void share.loadTreeChildren(rId, rel)}
          />
        </ScrollArea>

        <div
          id={PEER_SHARE_DROP_TARGET}
          data-file-drop-target
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-border/60 bg-card/50"
        >
          <PeerShareListView
            browse={browse}
            inTrash={inTrash}
            root={root}
            selection={selection}
            onSelectionChange={setSelection}
            onOpenDir={name => share.navigate(relJoin(dir, name))}
            onDownload={downloadEntry}
            onDownloadSelection={() => void downloadSelection()}
            onRename={entry => setNameDialog({ kind: 'rename', entry })}
            onCopy={entry => setNameDialog({ kind: 'copy', entry })}
            onDelete={names => deleteRels(names.map(name => (inTrash ? name : relJoin(dir, name))))}
            onRestore={restoreTrashEntry}
            onEmptyTrash={() => setEmptyTrashOpen(true)}
            onLoadMore={loadMore}
          />
          {depthLimitHit && (
            <p className="border-t border-border/50 px-3 py-1.5 text-ui-caption text-muted-foreground">
              {t('devices.share.depthLimit')}
            </p>
          )}
        </div>
      </div>

      <PeerShareTransferBar peerId={peerId} />

      <PeerShareDialogs
        pendingUpload={pendingUpload}
        onConfirmUpload={confirmUpload}
        onCancelUpload={() => setPendingUpload(null)}
        nameDialog={nameDialog}
        onSubmitName={submitNameDialog}
        onCloseName={() => setNameDialog(null)}
        emptyTrashOpen={emptyTrashOpen}
        onConfirmEmptyTrash={emptyTrash}
        onCloseEmptyTrash={() => setEmptyTrashOpen(false)}
      />
    </div>
  )
}

export default PeerShareTab
