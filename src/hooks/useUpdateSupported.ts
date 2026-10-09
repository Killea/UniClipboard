import { useEffect, useState } from 'react'
import { isUpdateSupported } from '@/api/updater'
import { createLogger } from '@/lib/logger'

const log = createLogger('update-supported')

/**
 * Whether this build ships with in-app update support.
 *
 * The backend answer is a compile-time constant — self-maintained
 * Linux/Windows builds always report `false` — so it is cached process-wide
 * after the first successful probe. `false` is returned while the probe is in
 * flight so consumers never render update controls enabled prematurely.
 */
export function useUpdateSupported(): boolean {
  const [supported, setSupported] = useState(cached ?? false)

  useEffect(() => {
    if (cached !== null) return
    let cancelled = false
    isUpdateSupported()
      .then(value => {
        cached = value
        if (!cancelled) setSupported(value)
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, '获取更新支持状态失败')
      })
    return () => {
      cancelled = true
    }
  }, [])

  return supported
}

let cached: boolean | null = null
