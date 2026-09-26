import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))

import {
  chapterAt,
  formatTimestamp,
  loadListeningProgress,
  loadReaderPublication,
  openReader,
  saveListeningProgress,
} from './reader'

describe('reader wrappers', () => {
  beforeEach(() => invoke.mockReset())

  it('loads the reader publication for an edition', async () => {
    invoke.mockResolvedValueOnce(null)
    await expect(loadReaderPublication('e')).resolves.toBeNull()
    expect(invoke).toHaveBeenCalledWith('reader_publication', { editionId: 'e' })
  })

  it('throws the backend error envelope', async () => {
    invoke.mockRejectedValueOnce({ code: 'x', message: 'bad' })
    await expect(loadReaderPublication('e')).rejects.toEqual({ code: 'x', message: 'bad' })
  })

  it('opens the reader window for an edition', async () => {
    invoke.mockResolvedValueOnce(null)
    await expect(openReader('e')).resolves.toBeNull()
    expect(invoke).toHaveBeenCalledWith('open_reader', { editionId: 'e' })
  })

  it('reads and writes listening progress', async () => {
    invoke.mockResolvedValueOnce(null)
    await expect(loadListeningProgress('e')).resolves.toBeNull()
    expect(invoke).toHaveBeenCalledWith('get_listening_progress', { editionId: 'e' })

    invoke.mockResolvedValueOnce({
      edition_id: 'e',
      position_seconds: 10,
      duration_seconds: 100,
    })
    await expect(saveListeningProgress('e', 10)).resolves.toEqual({
      edition_id: 'e',
      position_seconds: 10,
      duration_seconds: 100,
    })
    expect(invoke).toHaveBeenCalledWith('save_listening_progress', {
      editionId: 'e',
      positionSeconds: 10,
    })
  })
})

describe('formatTimestamp', () => {
  it('formats seconds as H:MM:SS', () => {
    expect(formatTimestamp(0)).toBe('0:00:00')
    expect(formatTimestamp(7)).toBe('0:00:07')
    expect(formatTimestamp(65)).toBe('0:01:05')
    expect(formatTimestamp(3723)).toBe('1:02:03')
    expect(formatTimestamp(23172)).toBe('6:26:12')
  })

  it('clamps non-finite input to zero', () => {
    expect(formatTimestamp(Number.NaN)).toBe('0:00:00')
    expect(formatTimestamp(-5)).toBe('0:00:00')
  })
})

describe('chapterAt', () => {
  const chapters = [
    { name: 'Intro', audio_start: 0, audio_end: 300 },
    { name: 'Middle', audio_start: 300, audio_end: 600 },
    { name: 'End', audio_start: 600, audio_end: 900 },
  ]

  it('returns the chapter containing the position', () => {
    expect(chapterAt(chapters, 0)).toBe(0)
    expect(chapterAt(chapters, 299.9)).toBe(0)
    expect(chapterAt(chapters, 300)).toBe(1)
    expect(chapterAt(chapters, 900)).toBe(2)
  })

  it('returns -1 for an empty chapter list', () => {
    expect(chapterAt([], 10)).toBe(-1)
  })
})
