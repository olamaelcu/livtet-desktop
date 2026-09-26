import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  canOpenInReader,
  loadEpubReaderPublication,
  openEpubReader,
  readReaderResource,
} from './read'

const manifestJson = JSON.stringify({
  metadata: { title: 'Test Book' },
  readingOrder: [{ href: 'ch01.xhtml', type: 'application/xhtml+xml' }],
})

const positionsJson = JSON.stringify([
  {
    href: 'ch01.xhtml',
    type: 'application/xhtml+xml',
    locations: { progression: 0, position: 1, totalProgression: 0 },
  },
])

function publicationPayload(overrides: { manifest?: string; positions?: string } = {}) {
  return {
    kind: 'Epub',
    edition_id: 'edition-1',
    title: 'Test Book',
    base_url: 'reader://localhost/edition-1/',
    manifest: overrides.manifest ?? manifestJson,
    positions: overrides.positions ?? positionsJson,
  }
}

describe('openEpubReader', () => {
  beforeEach(() => invoke.mockReset())

  it('invokes open_reader with the snake_case edition id', async () => {
    invoke.mockResolvedValueOnce(undefined)
    await openEpubReader('edition-1')
    expect(invoke).toHaveBeenCalledWith('open_reader', { edition_id: 'edition-1' })
  })
})

describe('loadEpubReaderPublication', () => {
  beforeEach(() => invoke.mockReset())

  it('maps the IPC payload to camelCase with parsed JSON', async () => {
    invoke.mockResolvedValueOnce(publicationPayload())
    const result = await loadEpubReaderPublication('edition-1')
    expect(invoke).toHaveBeenCalledWith('reader_publication', { edition_id: 'edition-1' })
    expect(result.baseUrl).toBe('reader://localhost/edition-1/')
    expect(result.manifest).toEqual(JSON.parse(manifestJson))
    expect(result.positions).toEqual(JSON.parse(positionsJson))
  })

  it('rejects an unparseable manifest', async () => {
    invoke.mockResolvedValueOnce(publicationPayload({ manifest: 'not-json{' }))
    await expect(loadEpubReaderPublication('edition-1')).rejects.toThrow(/manifest/)
  })

  it('rejects a manifest without a reading order', async () => {
    for (const manifest of ['null', '{}', '{"readingOrder": []}', '{"readingOrder": "x"}']) {
      invoke.mockResolvedValueOnce(publicationPayload({ manifest }))
      await expect(loadEpubReaderPublication('edition-1')).rejects.toThrow(/reading order/)
    }
  })

  it('rejects unparseable positions', async () => {
    invoke.mockResolvedValueOnce(publicationPayload({ positions: 'not-json{' }))
    await expect(loadEpubReaderPublication('edition-1')).rejects.toThrow(/positions/)
  })

  it('rejects non-array positions', async () => {
    invoke.mockResolvedValueOnce(publicationPayload({ positions: '{"positions": []}' }))
    await expect(loadEpubReaderPublication('edition-1')).rejects.toThrow(/positions/)
  })

  it('rejects a missing publication', async () => {
    invoke.mockResolvedValueOnce(null)
    await expect(loadEpubReaderPublication('edition-1')).rejects.toThrow(/No publication found/)
  })

  it('rejects the Audiobook variant', async () => {
    invoke.mockResolvedValueOnce({
      kind: 'Audiobook',
      edition_id: 'edition-1',
      title: 'Test Book',
      duration_seconds: 60,
      chapters: [],
      audio_url: 'http://127.0.0.1:9/audio/edition-1?t=t',
    })
    await expect(loadEpubReaderPublication('edition-1')).rejects.toThrow(/not an EPUB/)
  })
})

describe('readReaderResource', () => {
  beforeEach(() => invoke.mockReset())

  it('invokes reader_resource with the edition id and href', async () => {
    invoke.mockResolvedValueOnce('<html></html>')
    const text = await readReaderResource('edition-1', 'OEBPS/ch01.xhtml')
    expect(invoke).toHaveBeenCalledWith('reader_resource', {
      edition_id: 'edition-1',
      href: 'OEBPS/ch01.xhtml',
    })
    expect(text).toBe('<html></html>')
  })
})

describe('canOpenInReader', () => {
  it('accepts a present EPUB file', () => {
    expect(
      canOpenInReader({ file_path: '/books/test.epub', file_format: 'epub', file_status: 'ok' }),
    ).toBe(true)
  })

  it('matches the EPUB format case-insensitively', () => {
    expect(canOpenInReader({ file_path: '/books/test.EPUB', file_format: 'EPUB' })).toBe(true)
  })

  it('falls back to the file extension when no format is recorded', () => {
    expect(canOpenInReader({ file_path: '/books/test.epub' })).toBe(true)
  })

  it('rejects non-EPUB formats', () => {
    expect(canOpenInReader({ file_path: '/books/test.pdf', file_format: 'pdf' })).toBe(false)
  })

  it('rejects files marked missing', () => {
    expect(
      canOpenInReader({
        file_path: '/books/test.epub',
        file_format: 'epub',
        file_status: 'missing',
      }),
    ).toBe(false)
  })

  it('rejects absent files', () => {
    expect(canOpenInReader(null)).toBe(false)
    expect(canOpenInReader(undefined)).toBe(false)
    expect(canOpenInReader({})).toBe(false)
  })
})
