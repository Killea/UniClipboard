import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { PeerShareEntryDto } from '@/api/daemon/remote-share'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { formatFileSize } from '@/utils/formatters'

export interface PendingUpload {
  paths: string[]
  itemCount: number
  totalBytes: number
  peerName: string
  /** `label:rel` destination the peer will see. */
  destination: string
}

type NameDialogKind =
  | { kind: 'mkdir' }
  | { kind: 'rename'; entry: PeerShareEntryDto }
  | { kind: 'copy'; entry: PeerShareEntryDto }

interface PeerShareDialogsProps {
  pendingUpload: PendingUpload | null
  onConfirmUpload: () => void
  onCancelUpload: () => void
  nameDialog: NameDialogKind | null
  onSubmitName: (value: string) => void
  onCloseName: () => void
  emptyTrashOpen: boolean
  onConfirmEmptyTrash: () => void
  onCloseEmptyTrash: () => void
}

/**
 * All modal surfaces of the peer Files tab: the mandatory drop/upload
 * confirmation (spec §11.6 — count + bytes + peer + destination), the single
 * name-entry dialog shared by mkdir/rename/copy-target, and the empty-trash
 * confirmation (the only permanent delete, §11.7).
 */
const PeerShareDialogs: React.FC<PeerShareDialogsProps> = ({
  pendingUpload,
  onConfirmUpload,
  onCancelUpload,
  nameDialog,
  onSubmitName,
  onCloseName,
  emptyTrashOpen,
  onConfirmEmptyTrash,
  onCloseEmptyTrash,
}) => {
  const { t } = useTranslation()
  const [nameValue, setNameValue] = useState('')

  useEffect(() => {
    if (nameDialog?.kind === 'rename') setNameValue(nameDialog.entry.name)
    else if (nameDialog?.kind === 'copy') setNameValue('')
    else setNameValue('')
  }, [nameDialog])

  const nameTitle =
    nameDialog?.kind === 'mkdir'
      ? t('devices.share.dialogs.mkdirTitle')
      : nameDialog?.kind === 'rename'
        ? t('devices.share.dialogs.renameTitle')
        : t('devices.share.dialogs.copyTitle')
  const namePlaceholder =
    nameDialog?.kind === 'copy' ? 'subdir/path' : t('devices.share.dialogs.namePlaceholder')

  return (
    <>
      {/* upload confirmation — required before any bytes move (§11.6) */}
      <AlertDialog open={!!pendingUpload} onOpenChange={open => !open && onCancelUpload()}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('devices.share.uploadConfirm.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.share.uploadConfirm.body', {
                count: pendingUpload?.itemCount ?? 0,
                size: formatFileSize(pendingUpload?.totalBytes ?? 0),
                peer: pendingUpload?.peerName ?? '',
                destination: pendingUpload?.destination ?? '',
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('devices.share.uploadConfirm.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={onConfirmUpload}>
              {t('devices.share.uploadConfirm.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* mkdir / rename / copy-target name entry */}
      <Dialog open={!!nameDialog} onOpenChange={open => !open && onCloseName()}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{nameTitle}</DialogTitle>
          </DialogHeader>
          <form
            onSubmit={e => {
              e.preventDefault()
              if (nameValue.trim()) onSubmitName(nameValue.trim())
            }}
            className="flex flex-col gap-4"
          >
            <Input
              autoFocus
              value={nameValue}
              onChange={e => setNameValue(e.target.value)}
              placeholder={namePlaceholder}
            />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={onCloseName}>
                {t('devices.share.dialogs.cancel')}
              </Button>
              <Button type="submit" disabled={!nameValue.trim()}>
                {t('devices.share.dialogs.confirm')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {/* empty trash — the only permanent deletion (§11.7) */}
      <AlertDialog open={emptyTrashOpen} onOpenChange={open => !open && onCloseEmptyTrash()}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('devices.share.emptyTrash.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.share.emptyTrash.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('devices.share.emptyTrash.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={onConfirmEmptyTrash}
            >
              {t('devices.share.emptyTrash.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

export default PeerShareDialogs
