import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The progress store talks to the generated bindings; stub the commands so
// tests stay offline and can assert exactly what was sent.
const saveReaderPosition = vi.fn()
const getReaderPosition = vi.fn()
vi.mock('$lib/bindings', () => ({
  commands: {
    saveReaderPosition: (...args: unknown[]) => saveReaderPosition(...args),
    getReaderPosition: (...args: unknown[]) => getReaderPosition(...args),
  },
}))

import { debouncedSave, loadProgress, saveProgress } from './progress'

describe('progress store', () => {
  const editionId = '01ABCDEF'

  beforeEach(() => {
    saveReaderPosition.mockReset()
    saveReaderPosition.mockResolvedValue({ status: 'ok', data: null })
    getReaderPosition.mockReset()
    getReaderPosition.mockResolvedValue({ status: 'ok', data: null })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('persists an EPUB locator to the backend', async () => {
    await saveProgress(editionId, { kind: 'epub', epub: { locator: '{"href":"c1"}' } })
    expect(saveReaderPosition).toHaveBeenCalledWith(
      editionId,
      'epub',
      '{"href":"c1"}',
      null,
      null,
      null,
    )
  })

  it('persists a PDF page number to the backend', async () => {
    await saveProgress(editionId, { kind: 'pdf', pdf: { pageNumber: 7 } })
    expect(saveReaderPosition).toHaveBeenCalledWith(editionId, 'pdf', null, 7, null, null)
  })

  it('persists an audiobook position to the backend', async () => {
    await expect(
      saveProgress(editionId, { kind: 'audio', audio: { positionSeconds: 42.5 } }),
    ).resolves.toBe(true)
    expect(saveReaderPosition).toHaveBeenCalledWith(editionId, 'audio', null, null, 42.5, null)
  })

  it('reads a PDF page number back from the database', async () => {
    getReaderPosition.mockResolvedValue({
      status: 'ok',
      data: { kind: 'pdf', locator: null, page_number: 9 },
    })

    const loaded = await loadProgress(editionId)
    expect(loaded?.kind).toBe('pdf')
    expect(loaded?.pdf?.pageNumber).toBe(9)
    expect(getReaderPosition).toHaveBeenCalledWith(editionId)
  })

  it('reads an EPUB locator back from the database', async () => {
    getReaderPosition.mockResolvedValue({
      status: 'ok',
      data: { kind: 'epub', locator: '{"href":"c3"}', page_number: null },
    })

    const loaded = await loadProgress(editionId)
    expect(loaded?.kind).toBe('epub')
    expect(loaded?.epub?.locator).toBe('{"href":"c3"}')
  })

  it('reads an audiobook position back from the database', async () => {
    getReaderPosition.mockResolvedValue({
      status: 'ok',
      data: { kind: 'audio', locator: null, page_number: null, position_seconds: 42.5 },
    })

    const loaded = await loadProgress(editionId)
    expect(loaded?.kind).toBe('audio')
    expect(loaded?.audio?.positionSeconds).toBe(42.5)
  })

  it('returns null when the database has no position', async () => {
    expect(await loadProgress(editionId)).toBeNull()
    expect(getReaderPosition).toHaveBeenCalledWith(editionId)
  })

  it('returns null when the backend read fails', async () => {
    getReaderPosition.mockRejectedValue(new Error('offline'))
    await expect(loadProgress(editionId)).resolves.toBeNull()
  })

  it('reports a failed backend write instead of throwing', async () => {
    saveReaderPosition.mockRejectedValue(new Error('offline'))
    await expect(saveProgress(editionId, { kind: 'pdf', pdf: { pageNumber: 4 } })).resolves.toBe(
      false,
    )
  })

  it('debounces rapid saves to a single backend write with the latest value', async () => {
    vi.useFakeTimers()
    let page = 1
    const save = debouncedSave(
      () => editionId,
      () => ({ kind: 'pdf', pdf: { pageNumber: page } }),
      250,
    )

    page = 2
    save()
    page = 3
    save()
    page = 5
    save()

    expect(saveReaderPosition).not.toHaveBeenCalled()
    vi.advanceTimersByTime(250)
    await vi.runAllTimersAsync()

    expect(saveReaderPosition).toHaveBeenCalledTimes(1)
    expect(saveReaderPosition).toHaveBeenCalledWith(editionId, 'pdf', null, 5, null, null)
  })

  it('persists annotations as a JSON string', async () => {
    const annotations = [{ id: 'a1', text: 'Key point', pageNumber: 3 }]
    await saveProgress(editionId, { kind: 'pdf', pdf: { pageNumber: 3 }, annotations })

    expect(saveReaderPosition).toHaveBeenCalledWith(
      editionId,
      'pdf',
      null,
      3,
      null,
      JSON.stringify(annotations),
    )
  })

  it('reads annotations back from the database', async () => {
    const annotations = [{ id: 'a1', text: 'Key point', locator: '{"href":"c2"}' }]
    getReaderPosition.mockResolvedValue({
      status: 'ok',
      data: {
        kind: 'epub',
        locator: '{"href":"c2"}',
        page_number: null,
        position_seconds: null,
        annotations: JSON.stringify(annotations),
      },
    })

    const loaded = await loadProgress(editionId)
    expect(loaded?.annotations).toEqual(annotations)
  })

  it('ignores malformed annotation JSON rather than failing the load', async () => {
    getReaderPosition.mockResolvedValue({
      status: 'ok',
      data: {
        kind: 'pdf',
        locator: null,
        page_number: 2,
        position_seconds: null,
        annotations: 'not json',
      },
    })

    const loaded = await loadProgress(editionId)
    expect(loaded?.pdf?.pageNumber).toBe(2)
    expect(loaded?.annotations).toBeUndefined()
  })
})
