import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listen(...args),
}))

import {
  type ImportRow,
  importErrorMessage,
  importFile,
  importFiles,
  listenToImportEvents,
  mergeBatchResult,
  reduceImportEvents,
  relinkEditionFile,
} from './import'

const imported = {
  work_id: 'w',
  edition_id: 'e',
  title: 'T',
  isbns: [],
  duplicate: false,
}

describe('importFiles', () => {
  beforeEach(() => {
    invoke.mockReset()
    listen.mockReset()
  })

  it('short-circuits null and empty input without IPC', async () => {
    expect(await importFiles(null)).toEqual({ files: [], imported: 0, duplicated: 0, failed: 0 })
    expect(await importFiles([])).toEqual({ files: [], imported: 0, duplicated: 0, failed: 0 })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('invokes import_files with the filtered paths, defaulting to link mode', async () => {
    invoke.mockResolvedValueOnce({ files: [], imported: 0, duplicated: 0, failed: 0 })
    await importFiles(['a.epub', ''])
    expect(invoke).toHaveBeenCalledWith('import_files', { paths: ['a.epub'], mode: 'link' })
  })

  it('passes an explicit copy mode through', async () => {
    invoke.mockResolvedValueOnce({ files: [], imported: 0, duplicated: 0, failed: 0 })
    await importFiles(['a.epub'], 'copy')
    expect(invoke).toHaveBeenCalledWith('import_files', { paths: ['a.epub'], mode: 'copy' })
  })

  it('treats an all-empty path list as a no-op', async () => {
    expect(await importFiles([''])).toEqual({ files: [], imported: 0, duplicated: 0, failed: 0 })
    expect(invoke).not.toHaveBeenCalled()
  })
})

describe('importFile', () => {
  beforeEach(() => invoke.mockReset())

  it('unwraps the success envelope', async () => {
    invoke.mockResolvedValueOnce(imported)
    await expect(importFile('a.epub')).resolves.toEqual(imported)
  })

  it('throws the error envelope', async () => {
    invoke.mockRejectedValueOnce({ code: 'parse', message: 'bad' })
    await expect(importFile('a.epub')).rejects.toEqual({ code: 'parse', message: 'bad' })
  })
})

describe('relinkEditionFile', () => {
  beforeEach(() => invoke.mockReset())

  it('unwraps the refreshed file', async () => {
    const file = { file_path: '/data/books/h-a.epub', file_status: 'ok' }
    invoke.mockResolvedValueOnce(file)
    await expect(relinkEditionFile('e', '/books/a.epub')).resolves.toEqual(file)
    expect(invoke).toHaveBeenCalledWith('relink_edition_file', {
      editionId: 'e',
      path: '/books/a.epub',
    })
  })

  it('throws the error envelope', async () => {
    invoke.mockRejectedValueOnce({ code: 'io', message: 'gone' })
    await expect(relinkEditionFile('e', '/books/a.epub')).rejects.toEqual({
      code: 'io',
      message: 'gone',
    })
  })
})

describe('importErrorMessage', () => {
  it('maps known codes and falls back to the message', () => {
    expect(importErrorMessage({ code: 'unsupported', message: 'x' })).toMatch(/no importer/i)
    expect(importErrorMessage({ code: 'mystery', message: 'details' })).toBe('details')
  })
})

describe('row reducers', () => {
  it('tracks started then finished events by path', () => {
    let rows: ImportRow[] = [{ path: 'a.epub', status: 'pending' }]
    rows = reduceImportEvents(rows, { index: 0, total: 1, path: 'a.epub', phase: 'started' })
    expect(rows[0].status).toBe('importing')
    rows = reduceImportEvents(rows, {
      index: 0,
      total: 1,
      path: 'a.epub',
      phase: 'finished',
      outcome: imported,
    })
    expect(rows[0].status).toBe('imported')
  })

  it('marks failures with a mapped message', () => {
    let rows: ImportRow[] = [{ path: 'a.epub', status: 'importing' }]
    rows = reduceImportEvents(rows, {
      index: 0,
      total: 1,
      path: 'a.epub',
      phase: 'finished',
      error: { code: 'io', message: 'nope' },
    })
    expect(rows[0].status).toBe('failed')
    expect(rows[0].message).toMatch(/could not be read/i)
  })

  it('merges an authoritative batch result', () => {
    const rows: ImportRow[] = [{ path: 'a.epub', status: 'importing' }]
    const merged = mergeBatchResult(rows, {
      files: [{ path: 'a.epub', outcome: { ...imported, duplicate: true }, error: null }],
      imported: 0,
      duplicated: 1,
      failed: 0,
    })
    expect(merged[0].status).toBe('duplicate')
  })
})

describe('listenToImportEvents', () => {
  beforeEach(() => listen.mockReset())

  it('cleans up when a later subscription fails', async () => {
    const firstUnlisten = vi.fn()
    listen.mockResolvedValueOnce(firstUnlisten)
    listen.mockRejectedValueOnce(new Error('boom'))

    await expect(listenToImportEvents({ batch: () => {}, file: () => {} })).rejects.toThrow('boom')
    expect(firstUnlisten).toHaveBeenCalledTimes(1)
  })
})
