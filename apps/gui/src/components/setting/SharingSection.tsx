import { FolderOpen, FolderPlus, Lock, Trash2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  remoteShareApi,
  type RemoteShareRootInputDto,
  type RemoteShareStateDto,
} from '@/api/daemon/remote-share'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { SettingRow } from '@/components/setting/SettingRow'
import { Button, Checkbox, Input, Switch } from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('sharing-section')

/**
 * Settings「共享」group (spec §11.5) — the frontend editor for the local
 * `/remote-share/*` routes. Manages THIS machine's exposure: per-root path,
 * label, read/write/delete grants, enabled flag, and the global suspend
 * switch. Deliberately absent from the peer「文件」tab (§11.4).
 *
 * While the encryption session is locked the daemon answers 200 with
 * `locked: true` and no roots — the section then shows an honest
 * "unlock to manage" state instead of pretending empty.
 */
const SharingSection: React.FC = () => {
  const { t } = useTranslation()
  const [state, setState] = useState<RemoteShareStateDto | null>(null)
  const [roots, setRoots] = useState<RemoteShareRootInputDto[]>([])
  const [dirty, setDirty] = useState(false)
  const [saving, setSaving] = useState(false)

  const reload = useCallback(() => {
    remoteShareApi
      .getLocalState()
      .then(next => {
        setState(next)
        setRoots(next.roots ?? [])
        setDirty(false)
      })
      .catch(err => {
        log.warn({ err }, 'remote-share state fetch failed')
        toast.error(t('settings.sharing.errors.loadFailed'))
      })
  }, [t])

  useEffect(reload, [reload])

  const patchRoot = (index: number, patch: Partial<RemoteShareRootInputDto>) => {
    setRoots(prev => prev.map((r, i) => (i === index ? { ...r, ...patch } : r)))
    setDirty(true)
  }

  const addRoot = useCallback(() => {
    void commands
      .pickDirectory()
      .then(path => {
        if (!path) return
        const label = path.split(/[\\/]/).filter(Boolean).pop() ?? path
        setRoots(prev => [
          ...prev,
          { path, label, read: true, write: false, delete: false, enabled: true },
        ])
        setDirty(true)
      })
      .catch(err => log.warn({ err }, 'pick_directory failed'))
  }, [])

  const save = useCallback(() => {
    setSaving(true)
    remoteShareApi
      .replaceRoots(roots)
      .then(() => {
        setDirty(false)
        toast.success(t('settings.sharing.saved'))
      })
      .catch(err => {
        log.warn({ err }, 'remote-share roots update failed')
        toast.error(t('settings.sharing.errors.saveFailed'))
      })
      .finally(() => setSaving(false))
  }, [roots, t])

  const toggleSuspend = useCallback(
    (suspended: boolean) => {
      remoteShareApi
        .setSuspended(suspended)
        .then(() => setState(prev => (prev ? { ...prev, suspended } : prev)))
        .catch(err => {
          log.warn({ err }, 'remote-share suspend toggle failed')
          toast.error(t('settings.sharing.errors.saveFailed'))
        })
    },
    [t]
  )

  if (state === null) {
    return (
      <p className="py-4 text-ui-caption text-muted-foreground">{t('devices.share.loading')}</p>
    )
  }

  if (state.locked) {
    return (
      <SettingGroup title={t('settings.sharing.title')}>
        <div className="flex items-center gap-3 py-4 text-muted-foreground">
          <Lock className="size-4" />
          <p className="text-ui-body">{t('settings.sharing.locked')}</p>
        </div>
      </SettingGroup>
    )
  }

  return (
    <>
      <SettingGroup title={t('settings.sharing.title')}>
        <SettingRow
          label={t('settings.sharing.suspend.label')}
          description={t('settings.sharing.suspend.description')}
        >
          <Switch
            checked={state.suspended ?? false}
            onCheckedChange={toggleSuspend}
            aria-label={t('settings.sharing.suspend.label')}
          />
        </SettingRow>
      </SettingGroup>

      <SettingGroup title={t('settings.sharing.rootsTitle')}>
        {roots.length === 0 && (
          <p className="py-3 text-ui-caption text-muted-foreground">
            {t('settings.sharing.empty')}
          </p>
        )}
        {roots.map((root, i) => (
          <div
            key={root.id ?? `new-${i}`}
            className="flex flex-col gap-2 border-b border-border/40 py-3 last:border-0"
          >
            <div className="flex items-center gap-2">
              <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate font-mono text-ui-caption" title={root.path}>
                {root.path}
              </span>
              <Switch
                checked={root.enabled ?? true}
                onCheckedChange={enabled => patchRoot(i, { enabled })}
                aria-label={t('settings.sharing.enabled')}
              />
              <Button
                variant="ghost"
                size="icon"
                className="size-7 text-destructive hover:text-destructive"
                aria-label={t('settings.sharing.remove')}
                onClick={() => {
                  setRoots(prev => prev.filter((_, j) => j !== i))
                  setDirty(true)
                }}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
            <div className="flex flex-wrap items-center gap-3 pl-6">
              <Input
                value={root.label}
                onChange={e => patchRoot(i, { label: e.target.value })}
                placeholder={t('settings.sharing.labelPlaceholder')}
                className="h-7 w-40"
              />
              {(['read', 'write', 'delete'] as const).map(grant => (
                <label
                  key={grant}
                  className="flex items-center gap-1.5 text-ui-caption text-muted-foreground"
                >
                  <Checkbox
                    checked={root[grant]}
                    onCheckedChange={checked => patchRoot(i, { [grant]: checked === true })}
                    className="size-3.5"
                  />
                  {t(`settings.sharing.grants.${grant}`)}
                </label>
              ))}
            </div>
          </div>
        ))}
        <div className="flex items-center gap-2 pt-3">
          <Button variant="outline" size="sm" onClick={addRoot}>
            <FolderPlus className="size-3.5" />
            {t('settings.sharing.add')}
          </Button>
          <span className="flex-1" />
          <Button size="sm" disabled={!dirty || saving} onClick={save}>
            {t('settings.sharing.apply')}
          </Button>
        </div>
      </SettingGroup>
    </>
  )
}

export default SharingSection
