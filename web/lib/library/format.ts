import type { CatalogError } from '../bindings'

/** Human-readable byte size, e.g. `1.5 MB`. */
export function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  const rendered = unit === 0 ? String(value) : value.toFixed(value >= 10 ? 0 : 1)
  return `${rendered} ${units[unit]}`
}

/** Trailing path segment, tolerating both POSIX and Windows separators. */
export function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path
}

/** Strip a leading `urn:<scheme>:` so identifiers show just their value. */
export function formatIdentifier(value: string): string {
  const match = /^urn:[^:]+:(.+)$/i.exec(value.trim())
  return match ? match[1] : value
}

/**
 * Rebuild the stored form of an edited identifier.
 *
 * The edit form shows the display form `formatIdentifier` produces, so saving
 * has to put the `urn:` prefix back — under the (possibly edited) kind. A value
 * the reader typed out in full is stored verbatim, and an entry that never was
 * a URN stays one plain string.
 */
export function composeIdentifier(kind: string, displayValue: string, storedValue: string): string {
  const value = displayValue.trim()
  if (/^urn:/i.test(value)) return value
  return /^urn:[^:]+:/i.test(storedValue.trim()) ? `urn:${kind.trim()}:${value}` : value
}

/** Human-readable text for a failed catalog command. */
export function describeCatalogError(error: CatalogError): string {
  // The generated ERROR union keeps every variant optional (`?: never`), so each
  // branch narrows on the presence of its own member.
  if (error.InvalidId) return `Invalid edition id: ${error.InvalidId.id}`
  if (error.InvalidIdentifier) return error.InvalidIdentifier.message
  if (error.IdentifierNotFound) return 'That identifier is no longer on this book'
  if (error.IdentifierConflict) {
    const { value, existing_kind } = error.IdentifierConflict
    return `Value ${value} is already stored as ${existing_kind}`
  }
  return error.Database?.message ?? 'The catalog write failed'
}
