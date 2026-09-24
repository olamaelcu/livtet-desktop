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
