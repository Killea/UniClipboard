import { memo } from 'react'
import type { SpaceMember } from '@/api/daemon/members'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import PeerDetailPanel from '@/components/device/PeerDetailPanel'
import { useSettingSelector } from '@/hooks/useSetting'

function PeerDetailPanelContainer(props: {
  deviceId: string
  device: SpaceMember
  status?: DeviceRowStatus
  /** Sidebar deep-link lands directly on the「文件」tab (spec §11.1). */
  initialTab?: 'files'
  onUnpair: (peerId: string) => void
}) {
  const globalSyncOff = useSettingSelector(({ setting }) => setting?.sync.syncEnabled === false)
  const globalFileSyncOff = useSettingSelector(
    ({ setting }) => setting?.fileSync?.fileSyncEnabled === false
  )
  const lanOnlyActive = useSettingSelector(
    ({ setting }) => setting?.network?.allowRelayFallback === false
  )
  return (
    <PeerDetailPanel
      {...props}
      globalSyncOff={globalSyncOff}
      globalFileSyncOff={globalFileSyncOff}
      lanOnlyActive={lanOnlyActive}
    />
  )
}

export default memo(PeerDetailPanelContainer)
