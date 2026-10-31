import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))

const { errorToast } = vi.hoisted(() => ({ errorToast: vi.fn() }))
vi.mock('svelte-sonner', () => ({ toast: { error: errorToast } }))

import {
  applyEnrichment,
  type EnrichmentPreview,
  type EnrichmentSummary,
  enrichDisabledReason,
  previewEnrichment,
  previewIsEmpty,
  reportEnrichmentError,
  summaryMessage,
} from './enrich'

function preview(overrides: Partial<EnrichmentPreview> = {}): EnrichmentPreview {
  return {
    sources: ['openlibrary'],
    added: [],
    unioned: [],
    conflicts: [],
    found: [],
    cover: null,
    ...overrides,
  }
}

function summary(overrides: Partial<EnrichmentSummary> = {}): EnrichmentSummary {
  return {
    edition_id: 'e1',
    sources: ['openlibrary'],
    mode: 'union',
    fields_added: 0,
    fields_overwritten: 0,
    fields_unioned: 0,
    identifiers_persisted: 0,
    cover_stored: true,
    ...overrides,
  }
}

describe('previewIsEmpty', () => {
  it('is empty when the source answered with nothing', () => {
    expect(previewIsEmpty(preview({ sources: ['openlibrary', 'wikidata'] }))).toBe(true)
  })

  it.each([
    ['an added scalar', { added: [{ field: 'title', value: 'Dune' }] }],
    ['a unionable list', { unioned: [{ field: 'subjects', values: ['SF'] }] }],
    ['a conflict', { conflicts: [{ field: 'title', current: 'A', incoming: 'B' }] }],
    ['a found identifier', { found: [{ kind: 'isbn', value: '9780441013593' }] }],
    ['a cover offer', { cover: { mime: 'image/jpeg', storable: true } }],
  ])('is not empty with %s', (_label, overrides) => {
    expect(previewIsEmpty(preview(overrides as Partial<EnrichmentPreview>))).toBe(false)
  })
})

describe('enrichDisabledReason', () => {
  const reason = 'Add an identifier (such as an ISBN) to this edition before enriching it.'

  it('explains an edition with no identifiers at all', () => {
    expect(enrichDisabledReason([])).toBe(reason)
  })

  it('explains an edition whose detail has not loaded', () => {
    expect(enrichDisabledReason(undefined)).toBe(reason)
  })

  it('explains an edition whose only identifiers are blank', () => {
    expect(
      enrichDisabledReason([
        { kind: 'isbn', value: '' },
        { kind: 'oclc', value: '   ' },
      ]),
    ).toBe(reason)
  })

  it('allows enrichment once one identifier carries a value', () => {
    expect(
      enrichDisabledReason([
        { kind: 'isbn', value: '' },
        { kind: 'isbn', value: '9780' },
      ]),
    ).toBe(null)
  })
})

describe('summaryMessage', () => {
  it('counts each kind of change', () => {
    expect(
      summaryMessage(summary({ fields_added: 2, fields_overwritten: 1, identifiers_persisted: 1 })),
    ).toBe('Added 2 fields, updated 1 field, linked 1 identifier.')
  })

  it('singularizes a single change', () => {
    expect(summaryMessage(summary({ fields_added: 1 }))).toBe('Added 1 field.')
  })

  it('reports appended list members as extended lists', () => {
    expect(summaryMessage(summary({ fields_unioned: 3 }))).toBe('Extended 3 lists.')
  })

  it('says so when the mode wrote nothing', () => {
    expect(summaryMessage(summary())).toBe('Applied with no field changes.')
  })

  it('notes a cover that could not be written', () => {
    expect(summaryMessage(summary({ fields_added: 2, cover_stored: false }))).toBe(
      'Added 2 fields; the cover could not be stored.',
    )
  })
})

describe('invokers', () => {
  beforeEach(() => {
    invoke.mockReset()
  })

  it('unwraps a successful preview', async () => {
    const payload = preview({ added: [{ field: 'title', value: 'Dune' }] })
    invoke.mockResolvedValue(payload)

    await expect(previewEnrichment('e1')).resolves.toEqual(payload)
    expect(invoke).toHaveBeenCalledWith('preview_enrichment', { editionId: 'e1' })
  })

  it('rethrows the PluginError a preview rejects with, unchanged', async () => {
    const error = { Invalid: { message: 'no edition e1' } }
    invoke.mockRejectedValue(error)

    await expect(previewEnrichment('e1')).rejects.toBe(error)
  })

  it('sends the chosen mode to apply and unwraps the summary', async () => {
    const payload = summary({ mode: 'overwrite', fields_overwritten: 1 })
    invoke.mockResolvedValue(payload)

    await expect(applyEnrichment('e1', 'overwrite')).resolves.toEqual(payload)
    expect(invoke).toHaveBeenCalledWith('apply_enrichment', { editionId: 'e1', mode: 'overwrite' })
  })

  it('rethrows the PluginError an apply rejects with, unchanged', async () => {
    const error = { Invalid: { message: 'preview expired; run Enrich again' } }
    invoke.mockRejectedValue(error)

    await expect(applyEnrichment('e1', 'fill')).rejects.toBe(error)
  })
})

describe('reportEnrichmentError', () => {
  beforeEach(() => {
    errorToast.mockReset()
  })

  it('offers the failure for copying, for longer than a plain toast', () => {
    reportEnrichmentError({ Invalid: { message: 'no enricher answered' } })

    const [message, options] = errorToast.mock.calls[0] as [string, { duration: number }]
    expect(message).toContain('Enrichment failed')
    expect(message).toContain('no enricher answered')
    expect(options.duration).toBeGreaterThan(4000)
  })

  it('writes the whole message to the clipboard when Copy is clicked', async () => {
    const writeText = vi.fn(async () => {})
    vi.stubGlobal('navigator', { clipboard: { writeText } })

    reportEnrichmentError({ Invalid: { message: 'preview expired; run Enrich again' } })
    const [, options] = errorToast.mock.calls[0] as [
      string,
      { action: { label: string; onClick: () => void } },
    ]

    expect(options.action.label).toBe('Copy')
    options.action.onClick()

    await vi.waitFor(() => expect(writeText).toHaveBeenCalledTimes(1))
    expect(writeText.mock.calls[0][0]).toContain('preview expired; run Enrich again')
    vi.unstubAllGlobals()
  })
})
