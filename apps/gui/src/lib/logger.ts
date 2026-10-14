import pino from 'pino'
import type { Level, LogEvent } from 'pino'
import { writeDiagnosticLog } from '@/observability/diagnostics'
import { appendLocalLog } from '@/observability/localLog'
import { redactSensitiveArgs } from '@/observability/redaction'
import { traceManager } from '@/observability/trace'

function stringifyArg(value: unknown, includeStack = false): string {
  if (value instanceof Error) {
    const base = `${value.name}: ${value.message}`
    return includeStack && value.stack ? value.stack : base
  }
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean' || value == null) return String(value)
  try {
    return JSON.stringify(redactSensitiveArgs(value))
  } catch {
    return String(value)
  }
}

function transmitDiagnostics(level: Level, logEvent: LogEvent): void {
  // Build message from all arguments, applying redaction on each.
  const message = logEvent.messages.map(m => stringifyArg(redactSensitiveArgs(m), false)).join(' ')

  // Merge all child-logger bindings (e.g. { module: 'api' }) into a flat object.
  const context = Object.assign({}, ...logEvent.bindings) as Record<string, unknown>

  const traceId = traceManager.getCurrentTrace()?.traceId
  const attributes: Record<string, unknown> = {}
  for (const item of logEvent.messages) {
    const redacted = redactSensitiveArgs(item)
    if (typeof redacted === 'object' && redacted !== null && !Array.isArray(redacted)) {
      Object.assign(attributes, redacted)
    }
  }
  if (traceId) attributes.trace_id = traceId
  if (context.module) attributes.module = String(context.module)

  writeDiagnosticLog(level, message, Object.keys(attributes).length > 0 ? attributes : undefined)

  // Mirror the record to the local on-disk log. Independent of the telemetry gate that
  // governs the diagnostics provider above, so it is always captured for offline inspection.
  appendLocalLog({
    level,
    msg: message,
    module: context.module ? String(context.module) : undefined,
    traceId,
    attributes: Object.keys(attributes).length > 0 ? attributes : undefined,
    ts: logEvent.ts,
  })
}

const isDev = import.meta.env.DEV

/**
 * Application-wide pino logger.
 *
 * - In development: writes to the browser console (default pino/browser behaviour) so
 *   records are visible in DevTools, and forwards structured records to the diagnostics
 *   provider (gated at runtime by `setDiagnosticsEnabled`).
 * - In production: the webview has no visible console (devtools are disabled), so pino's
 *   direct console write is suppressed. The `transmit` path below is the single channel
 *   that feeds both the diagnostics provider and the local on-disk log; keeping the
 *   console quiet also means the local log's console hook captures only ad-hoc
 *   `console.error`/`console.warn` calls, so a pino record is never recorded twice.
 *
 * Prefer creating module-level child loggers via `createLogger('module-name')`
 * for structured context rather than adding prefix strings to messages.
 *
 * @example
 * ```ts
 * const log = createLogger('daemon-ws')
 * log.info('connected')
 * log.warn({ attempt: 3 }, 'reconnecting')
 * log.error({ err }, 'fatal connection error')
 * ```
 */
export const logger = pino({
  level: isDev ? 'debug' : 'info',
  browser: {
    // Suppress pino's direct console write in production. The webview has no visible
    // console, and this keeps the local log's console hook from double-capturing pino
    // records (they reach the local log exactly once, via the transmit path).
    ...(isDev ? {} : { write: () => undefined }),
    transmit: {
      level: 'info',
      send: transmitDiagnostics,
    },
  },
})

/**
 * Create a child logger bound to a named module.
 * The `module` field is forwarded as a diagnostic log attribute so logs can be
 * filtered by component in the diagnostics viewer.
 */
export function createLogger(module: string): pino.Logger {
  return logger.child({ module })
}
