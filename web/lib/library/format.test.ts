import { describe, expect, it } from 'vitest'
import { fileName, formatFileSize } from './format'

describe('formatFileSize', () => {
  it('renders bytes without a unit divisor', () => {
    expect(formatFileSize(512)).toBe('512 B')
  })

  it('scales to the largest fitting unit', () => {
    expect(formatFileSize(1024)).toBe('1.0 KB')
    expect(formatFileSize(1024 * 1024 * 1.5)).toBe('1.5 MB')
    expect(formatFileSize(1024 * 1024 * 20)).toBe('20 MB')
  })

  it('guards invalid input', () => {
    expect(formatFileSize(-1)).toBe('—')
    expect(formatFileSize(Number.NaN)).toBe('—')
  })
})

describe('fileName', () => {
  it('returns the trailing segment for posix and windows paths', () => {
    expect(fileName('/a/b/book.epub')).toBe('book.epub')
    expect(fileName('C:\\a\\book.epub')).toBe('book.epub')
  })
})
