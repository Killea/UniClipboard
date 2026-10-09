import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import appIcon from '@/assets/app-icon.png'
import { Badge } from '@/components/ui/badge'
import { createLogger } from '@/lib/logger'
import { SponsorsGroup } from './about/SponsorsGroup'

const log = createLogger('about-section')

function parseChannel(version: string): string {
  const match = version.match(/-(alpha|beta|rc)/)
  return match ? match[1] : 'stable'
}

function getChannelBadgeVariant(channel: string): 'outline' | 'secondary' {
  return channel === 'stable' ? 'secondary' : 'outline'
}

function getChannelLabel(channel: string): string {
  const labels: Record<string, string> = {
    alpha: 'Alpha',
    beta: 'Beta',
    rc: 'RC',
    stable: 'Stable',
  }
  return labels[channel] ?? channel
}

const AboutSection: React.FC = () => {
  const { t } = useTranslation()
  const [appVersion, setAppVersion] = useState<string>('')

  const channel = appVersion ? parseChannel(appVersion) : null

  useEffect(() => {
    let cancelled = false
    getVersion()
      .then(version => {
        if (!cancelled) setAppVersion(version)
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, 'Failed to get app version')
      })
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <div className="flex min-w-0 flex-col gap-8">
      <div className="flex min-w-0 flex-wrap items-center gap-4 px-1">
        <img src={appIcon} alt="" className="size-12 shrink-0 rounded-lg" />
        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-ui-section font-semibold">
              {t('settings.sections.about.appName')}
            </h2>
            {channel && (
              <Badge variant={getChannelBadgeVariant(channel)}>{getChannelLabel(channel)}</Badge>
            )}
          </div>
          <p className="text-ui-caption text-muted-foreground">
            {appVersion
              ? t('settings.sections.about.version', { version: appVersion })
              : t('settings.sections.about.version', { version: '...' })}
          </p>
          <p className="text-ui-caption text-muted-foreground">
            {t('settings.sections.about.selfMaintained')}
          </p>
        </div>
      </div>

      {/* Sponsors */}
      <SponsorsGroup />

      {/* Footer: links + copyright */}
      <div className="space-y-2.5 pt-1 text-center">
        <div className="flex justify-center gap-x-5 text-ui-body">
          <a
            href="https://github.com/Killea/UniClipboard"
            className="text-muted-foreground transition-colors hover:text-foreground"
            target="_blank"
            rel="noreferrer"
          >
            {t('settings.sections.about.links.repository')}
          </a>
        </div>
        <p className="text-ui-caption text-muted-foreground/80">
          {t('settings.sections.about.copyright')}
        </p>
      </div>
    </div>
  )
}

export default AboutSection
