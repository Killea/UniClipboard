/**
 * Local GUI log capture.
 *
 * The WebView's runtime errors normally go to Sentry only, which is gated by the user's
 * telemetry preference and therefore invisible in the offline log files. This module
 * mirrors every pino record and raw browser error into the desktop host's local
 * `uniclipboard-gui.json.<date>` file so the on-screen state can be diagnosed without a
 * remote sink. It is intentionally independent of the telemetry gate: local logging is
 * always on, unlike remote delivery.
 *
 * Records buffer in memory and flush to the host `append_gui_logs` command
 * fire-and-forget. A failed flush is dropped (never re-buffered), so a missing IPC bridge
 * can never grow the buffer without bound, and nothing in here may throw into
 * application code.
 */
import { invoke } from '@tauri-apps/api/core'
import { redactSensitiveArgs } from '@/observability/redaction'
import type { DiagnosticLogLevel } from './types'

/** A buffered record with a concrete ISO timestamp, ready to cross the bridge. */
interface BufferedEntry {
  level: DiagnosticLogLevel
  msg: string
  module?: string
  traceId?: string
  attributes?: Record<string, unknown>
  time: string
}

/** A record the callers append; `ts` (ms epoch) is optional and defaults to now. */
export interface LocalLogRecord {
  level: DiagnosticLogLevel
  msg: string
  module?: string
  traceId?: string
  attributes?: Record<string, unknown>
  ts?: number
}

const FLUSH_INTERVAL_MS = 5000
const FLUSH_THRESHOLD = 50
const MAX_BUFFER = 2000

const buffer: BufferedEntry[] = []
let flushTimer: number | undefined
let flushInFlight = false
let captureInstalled = false

function bridgeAvailable(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** Flush the current buffer to the host, fire-and-forget. Dropped if the bridge is absent. */
function flush(): void {
  if (flushInFlight || buffer.length === 0 || !bridgeAvailable()) {
    return
  }
  flushInFlight = true
  const batch = buffer.splice(0, buffer.length)
  void invoke('append_gui_logs', { entries: batch })
    .catch(() => undefined)
    .finally(() => {
      flushInFlight = false
    })
}

/**
 * Append one record to the local buffer, scheduling a flush. Never throws.
 *
 * @example
 * ```ts
 * appendLocalLog({ level: 'error', msg: 'boot failed', attributes: { err } })
 * ```
 */
export function appendLocalLog(record: LocalLogRecord): void {
  try {
    buffer.push({
      level: record.level,
      msg: record.msg,
      module: record.module,
      traceId: record.traceId,
      attributes: record.attributes,
      time: new Date(record.ts ?? Date.now()).toISOString(),
    })
    if (buffer.length > MAX_BUFFER) {
      buffer.splice(0, buffer.length - MAX_BUFFER)
    }
    if (buffer.length >= FLUSH_THRESHOLD) {
      flush()
      return
    }
    if (flushTimer === undefined && typeof window !== 'undefined') {
      flushTimer = window.setTimeout(() => {
        flushTimer = undefined
        flush()
      }, FLUSH_INTERVAL_MS)
    }
  } catch {
    // Local logging must never disturb the application.
  }
}

/** Stringify console-style arguments, keeping an Error's stack intact. */
function formatConsoleArgs(args: unknown[]): string {
  return args
    .map(arg => {
      if (arg instanceof Error) {
        return `${arg.name}: ${arg.message}${arg.stack ? `\n${arg.stack}` : ''}`
      }
      if (typeof arg === 'string') return arg
      try {
        return JSON.stringify(redactSensitiveArgs(arg))
      } catch {
        return String(arg)
      }
    })
    .join(' ')
}

/** Wrap a console method so its calls are also mirrored into the local buffer. */
function wrapConsole(method: 'error' | 'warn', level: DiagnosticLogLevel): void {
  const original = console[method].bind(console)
  let reentered = false
  console[method] = (...args: unknown[]) => {
    const result = original(...args)
    if (reentered) return result
    reentered = true
    try {
      appendLocalLog({ level, msg: formatConsoleArgs(args) })
    } catch {
      // never throw into application code
    } finally {
      reentered = false
    }
    return result
  }
}

/**
 * Install the global raw-error capture: `window` uncaught errors, unhandled promise
 * rejections, and `console.error`/`console.warn`. Routed into the same local buffer.
 * Idempotent; a no-op outside a browser-like environment.
 */
export function installLocalLogCapture(): void {
  if (typeof window === 'undefined' || captureInstalled) {
    return
  }
  captureInstalled = true

  window.addEventListener('error', event => {
    appendLocalLog({
      level: 'error',
      msg: event.message || 'uncaught error',
      attributes: redactSensitiveArgs({
        source: 'window.error',
        file: event.filename,
        line: event.lineno,
        column: event.colno,
        name: event.error?.name,
        stack: event.error?.stack,
      }) as Record<string, unknown>,
    })
  })

  window.addEventListener('unhandledrejection', event => {
    appendLocalLog({
      level: 'error',
      msg: 'unhandled promise rejection',
      attributes: redactSensitiveArgs({
        source: 'unhandledrejection',
        reason:
          event.reason instanceof Error
            ? (event.reason.stack ?? String(event.reason))
            : event.reason,
      }) as Record<string, unknown>,
    })
  })

  wrapConsole('error', 'error')
  wrapConsole('warn', 'warn')
}
