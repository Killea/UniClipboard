import { Bug, Layers, MessageSquare, Monitor, Settings, X } from 'lucide-react'
import React, { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation, useNavigate } from 'react-router'
import { updateDebugMode } from '@/api/daemon/diagnostics'
import { FeedbackDialog } from '@/components/feedback/FeedbackDialog'
import { toast } from '@/components/ui/toast'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useSettingSelector } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { diagnosticsConfigured } from '@/observability/diagnostics'

const log = createLogger('sidebar')

const NavButton: React.FC<{
  to: string
  icon: React.ComponentType<{ className?: string }>
  label: string
  isActive: boolean
  portalContainer: React.RefObject<HTMLElement | null>
  onClick?: (e: React.MouseEvent<HTMLAnchorElement>) => void
  'data-settings-icon'?: boolean
}> = ({
  to,
  icon: Icon,
  label,
  isActive,
  portalContainer,
  onClick,
  'data-settings-icon': dataSettingsIcon,
}) => {
  return (
    <TooltipProvider delay={0}>
      <Tooltip>
        <TooltipTrigger
          render={
            <Link
              data-tauri-drag-region="false"
              data-settings-icon={dataSettingsIcon || undefined}
              to={to}
              className={cn(
                'group relative block size-10 rounded-lg',
                !isActive && 'hover:bg-muted'
              )}
              onClick={
                onClick
                  ? e => {
                      e.preventDefault()
                      onClick(e)
                    }
                  : undefined
              }
            />
          }
        >
          {isActive && (
            <div
              aria-hidden
              className="absolute inset-0 rounded-lg bg-primary/10 dark:bg-primary/20"
            />
          )}
          <div
            className={cn(
              'relative z-10 flex size-10 items-center justify-center rounded-lg',
              isActive ? 'text-primary' : 'text-muted-foreground group-hover:text-primary'
            )}
          >
            <Icon className="size-5" />
          </div>
        </TooltipTrigger>
        <TooltipContent
          portalContainer={portalContainer}
          side="right"
          align="center"
          className="font-medium"
        >
          <p>{label}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

interface SidebarProps {
  className?: string
}

const Sidebar: React.FC<SidebarProps> = ({ className }) => {
  const sidebarRef = useRef<HTMLElement>(null)
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()
  const reloadSetting = useSettingSelector(context => context.reloadSetting)
  const debugMode = useSettingSelector(({ setting }) => setting?.general.debugMode)
  const [feedbackOpen, setFeedbackOpen] = useState(false)
  const [disablingDebug, setDisablingDebug] = useState(false)

  const navItems = [
    { to: '/history', icon: Layers, label: t('nav.history') },
    { to: '/devices', icon: Monitor, label: t('nav.devices') },
  ]

  const handleDisableDebugMode = async () => {
    if (disablingDebug) return
    setDisablingDebug(true)
    try {
      await updateDebugMode(false)
      await reloadSetting()
      toast.message(t('debugBadge.disabledToast'))
    } catch (error) {
      log.error({ err: error }, 'Failed to disable debug mode')
      toast.error(t('debugBadge.disableFailed'))
    } finally {
      setDisablingDebug(false)
    }
  }

  return (
    <>
      <aside
        ref={sidebarRef}
        data-tauri-drag-region
        className={cn(
          'relative z-10 w-14 h-full shrink-0 flex flex-col items-center py-4',
          'bg-transparent',
          className
        )}
      >
        {/* Main Navigation */}
        <div className="relative z-10 flex flex-col gap-3 w-full items-center">
          {navItems.map(item => (
            <NavButton
              key={item.to}
              to={item.to}
              icon={item.icon}
              label={item.label}
              isActive={location.pathname === item.to}
              portalContainer={sidebarRef}
            />
          ))}
        </div>

        <div data-tauri-drag-region className="flex-1 w-full min-h-0" />

        {/* Bottom Navigation */}
        <div className="relative z-10 flex flex-col gap-3 w-full items-center">
          {debugMode && (
            <TooltipProvider delay={0}>
              <Tooltip>
                <TooltipTrigger
                  render={
                    <div className="relative flex size-10 items-center justify-center rounded-lg border border-amber-500/25 bg-amber-500/10 text-amber-700 dark:text-amber-300" />
                  }
                >
                  <Bug className="size-5" />
                  <button
                    type="button"
                    aria-label={t('debugBadge.disable')}
                    data-tauri-drag-region="false"
                    className="absolute -right-1 -top-1 flex size-4 items-center justify-center rounded-full bg-background text-muted-foreground shadow-sm ring-1 ring-border transition-colors hover:text-foreground"
                    onClick={handleDisableDebugMode}
                    disabled={disablingDebug}
                  >
                    <X className="size-3" />
                  </button>
                </TooltipTrigger>
                <TooltipContent
                  portalContainer={sidebarRef}
                  side="right"
                  align="center"
                  className="max-w-64"
                >
                  <div className="space-y-1">
                    <p className="font-medium">{t('debugBadge.title')}</p>
                    <p className="text-ui-caption text-muted-foreground">
                      {t('debugBadge.description')}
                    </p>
                    <p className="text-ui-caption text-muted-foreground">
                      {t('debugBadge.restartHint')}
                    </p>
                  </div>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {diagnosticsConfigured && (
            <>
              <TooltipProvider delay={0}>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <button
                        type="button"
                        aria-label={t('nav.feedback')}
                        data-tauri-drag-region="false"
                        className="group relative size-10 rounded-lg hover:bg-muted"
                        onClick={() => setFeedbackOpen(true)}
                      />
                    }
                  >
                    <div
                      className={cn(
                        'relative z-10 flex size-10 items-center justify-center rounded-lg',
                        'text-muted-foreground group-hover:text-primary'
                      )}
                    >
                      <MessageSquare className="size-5" />
                    </div>
                  </TooltipTrigger>
                  <TooltipContent
                    portalContainer={sidebarRef}
                    side="right"
                    align="center"
                    className="font-medium"
                  >
                    <p>{t('nav.feedback')}</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <FeedbackDialog open={feedbackOpen} onOpenChange={setFeedbackOpen} />
            </>
          )}
          <NavButton
            to="/settings"
            icon={Settings}
            label={t('nav.settings')}
            isActive={location.pathname.startsWith('/settings')}
            portalContainer={sidebarRef}
            onClick={() => {
              if (location.pathname.startsWith('/settings')) return
              navigate('/settings')
            }}
          />
        </div>
      </aside>
    </>
  )
}

export default Sidebar
