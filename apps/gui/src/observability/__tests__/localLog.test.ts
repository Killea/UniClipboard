import { invoke } from '@tauri-apps/api/core'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

async function loadLocalLog() {
  vi.resetModules()
  return import('@/observability/localLog')
}

// Fill the buffer past the flush threshold to trigger an immediate (synchronous) flush.
async function forceFlush(appendLocalLog: (r: { level: string; msg: string }) => void, count = 50) {
  for (let i = 0; i < count; i++) {
    appendLocalLog({ level: 'info', msg: `filler-${i}` })
  }
}

function flushedEntries(): Record<string, unknown>[] {
  const calls = vi.mocked(invoke).mock.calls
  const payload = calls[calls.length - 1]?.[1] as { entries: Record<string, unknown>[] }
  return payload.entries
}

describe('localLog', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(invoke).mockResolvedValue(undefined)
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('flushes a full buffer to append_gui_logs with ISO-8601 timestamps', async () => {
    const { appendLocalLog } = await loadLocalLog()
    ;(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {}

    appendLocalLog({ level: 'error', msg: 'setup retry failed', module: 'setup', traceId: 't-1' })
    await forceFlush(appendLocalLog, 49)

    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith(
      'append_gui_logs',
      expect.objectContaining({ entries: expect.any(Array) })
    )
    const entries = flushedEntries()
    expect(entries).toHaveLength(50)
    const first = entries[0]
    expect(first).toMatchObject({
      level: 'error',
      msg: 'setup retry failed',
      module: 'setup',
      traceId: 't-1',
    })
    // time is a parseable ISO-8601 string produced by the WebView.
    expect(typeof first.time).toBe('string')
    expect(Number.isNaN(Date.parse(String(first.time)))).toBe(false)
  })

  it('does not flush when the Tauri bridge is unavailable (no re-buffer growth)', async () => {
    const { appendLocalLog } = await loadLocalLog()
    // __TAURI_INTERNALS__ is absent.
    for (let i = 0; i < 60; i++) {
      appendLocalLog({ level: 'error', msg: `err-${i}` })
    }
    expect(invoke).not.toHaveBeenCalled()
  })

  it('captures console.error via installLocalLogCapture and stays idempotent', async () => {
    const nativeErr = vi.fn()
    vi.spyOn(console, 'error').mockImplementation(nativeErr)
    const { installLocalLogCapture, appendLocalLog } = await loadLocalLog()
    ;(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {}

    installLocalLogCapture()
    installLocalLogCapture() // must not double-wrap

    console.error('something blew up')
    expect(nativeErr).toHaveBeenCalledWith('something blew up')

    await forceFlush(appendLocalLog, 49)
    const entries = flushedEntries()
    const matches = entries.filter(e => String(e.msg).includes('something blew up'))
    // Exactly one mirrored record — idempotency means no double-wrap.
    expect(matches).toHaveLength(1)
    expect(matches[0].level).toBe('error')
  })

  it('captures uncaught window errors with a redaction-safe source tag', async () => {
    const { installLocalLogCapture, appendLocalLog } = await loadLocalLog()
    ;(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {}
    installLocalLogCapture()

    window.dispatchEvent(new Event('error'))
    await forceFlush(appendLocalLog)
    const entries = flushedEntries()
    const match = entries.find(e => e.attributes?.source === 'window.error')
    expect(match).toBeDefined()
    expect(match?.level).toBe('error')
  })
})
