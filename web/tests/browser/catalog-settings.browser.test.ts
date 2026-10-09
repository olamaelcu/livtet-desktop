import '@awesome.me/webawesome/dist/components/badge/badge.js'
import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/card/card.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/option/option.js'
import '@awesome.me/webawesome/dist/components/select/select.js'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn() } }))
vi.mock('svelte-sonner', () => ({ toast }))

import CatalogSettingsFixture from './fixtures/CatalogSettingsFixture.svelte'

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaSelect = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

function catalog(id: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    title: `Catalog ${id}`,
    feed_url: `https://example.test/${id}/opds`,
    auth_kind: 'none',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

let catalogs: Array<Record<string, unknown>>
let presets: Array<Record<string, unknown>>
/** Replaced by tests that need confirm() to decline. */
let confirmResult: boolean
let originalConfirm: typeof window.confirm

beforeEach(() => {
  catalogs = [catalog('a')]
  presets = []
  confirmResult = true
  // confirm() would block the browser, so it is answered programmatically.
  originalConfirm = window.confirm
  window.confirm = () => confirmResult

  invoke.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case 'opds_catalogs_list':
        return catalogs
      case 'opds_default_catalogs':
        return presets
      case 'opds_catalogs_update':
      case 'opds_catalogs_create':
        return catalog('new')
      case 'opds_catalogs_remove':
        return null
      case 'opds_catalogs_test':
        return {
          title: 'Standard Ebooks',
          identifier: null,
          navigation: [],
          publications: [],
          next_href: null,
          has_search: false,
        }
      default:
        throw new Error(`unexpected command: ${command} ${JSON.stringify(args)}`)
    }
  })
})

afterEach(() => {
  window.confirm = originalConfirm
})

function rows(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.catalog-list .catalog-info strong')].map(
    (n) => n.textContent?.trim() ?? '',
  )
}

/** Svelte sets `label` as a property on wa-input, so the attribute is never
 * written and an attribute selector matches nothing. */
function inputByLabel(container: HTMLElement, label: string): WaInput | null {
  const inputs = [
    ...container.querySelectorAll<WaInput & { label?: string }>('.catalog-form wa-input'),
  ]
  return inputs.find((node) => node.label === label) ?? null
}

function authSelect(container: HTMLElement): WaSelect {
  return container.querySelector('.catalog-form wa-select') as WaSelect
}

function formButtons(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.form-actions wa-button')].map(
    (n) => n.textContent?.trim() ?? '',
  )
}

async function clickRowAction(container: HTMLElement, label: string) {
  const button = [...container.querySelectorAll<WaButton>('.catalog-list wa-button')].find(
    (n) => n.textContent?.trim() === label,
  )
  expect(button, `the row offers a ${label} action`).not.toBeUndefined()
  await button?.updateComplete
  button?.click()
}

async function submitForm(container: HTMLElement) {
  const button = container.querySelector<WaButton>('.form-actions wa-button')
  await button?.updateComplete
  button?.click()
}

async function argsOf(command: string): Promise<Record<string, unknown>> {
  await expect.poll(() => invoke.mock.calls.some(([c]) => c === command)).toBe(true)
  const call = invoke.mock.calls.find(([c]) => c === command)
  return (call?.[1] ?? {}) as Record<string, unknown>
}

test('lists the subscribed catalogs', async () => {
  catalogs = [catalog('a'), catalog('b')]
  const { container } = await render(CatalogSettingsFixture)

  await expect.poll(() => rows(container)).toEqual(['Catalog a', 'Catalog b'])
  expect(container.querySelector('p.empty')).toBeNull()
})

test('invites a first subscription when there are none', async () => {
  catalogs = []
  const { container } = await render(CatalogSettingsFixture)

  await expect.poll(() => container.querySelector('p.empty')).not.toBeNull()
  expect(rows(container)).toEqual([])
})

test('testing a catalog checks it and reports the feed it reached', async () => {
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])

  await clickRowAction(container, 'Test')

  expect(await argsOf('opds_catalogs_test')).toEqual({ id: 'a' })
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Connected: Standard Ebooks')
})

// Removal is destructive, so it must be confirmed first.
test('declining the removal prompt removes nothing', async () => {
  confirmResult = false
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])

  await clickRowAction(container, 'Remove')

  expect(invoke).not.toHaveBeenCalledWith('opds_catalogs_remove', expect.anything())
})

test('confirming the removal prompt removes that catalog', async () => {
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])

  await clickRowAction(container, 'Remove')

  expect(await argsOf('opds_catalogs_remove')).toEqual({ id: 'a' })
})

test('editing loads the catalog into the form and offers a way out', async () => {
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])
  expect(formButtons(container)).toEqual(['Add catalog'])

  await clickRowAction(container, 'Edit')

  await expect.poll(() => formButtons(container)).toEqual(['Save changes', 'Cancel'])
  expect(container.querySelector('.catalog-form h3')?.textContent?.trim()).toBe('Edit catalog')
})

test('cancelling an edit returns the form to adding', async () => {
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])
  await clickRowAction(container, 'Edit')
  await expect.poll(() => formButtons(container)).toEqual(['Save changes', 'Cancel'])

  const cancel = [...container.querySelectorAll<WaButton>('.form-actions wa-button')][1]
  await cancel.updateComplete
  cancel.click()

  await expect.poll(() => formButtons(container)).toEqual(['Add catalog'])
})

// The gem: on update, `undefined` auth leaves the stored secret alone while an
// explicit `none` clears it. updateCatalog maps undefined to null on the wire.
test('editing a basic-auth catalog without retyping keeps the stored secret', async () => {
  catalogs = [catalog('a', { auth_kind: 'basic' })]
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])

  await clickRowAction(container, 'Edit')
  await expect.poll(() => inputByLabel(container, 'Username')).not.toBeNull()
  expect(container.querySelector('.catalog-form p.hint')?.textContent).toContain(
    'keep the stored password',
  )

  await submitForm(container)

  expect(await argsOf('opds_catalogs_update')).toMatchObject({ id: 'a', auth: null })
})

test('switching an edited catalog to no auth clears the stored secret', async () => {
  catalogs = [catalog('a', { auth_kind: 'basic' })]
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])
  await clickRowAction(container, 'Edit')
  await expect.poll(() => inputByLabel(container, 'Username')).not.toBeNull()

  const select = authSelect(container)
  await select.updateComplete
  select.value = 'none'
  select.dispatchEvent(new Event('change', { bubbles: true }))

  await submitForm(container)

  expect(await argsOf('opds_catalogs_update')).toMatchObject({
    id: 'a',
    auth: { kind: 'none', username: null, password: null, token: null },
  })
})

test('retyping a password on an edit sends the new credentials', async () => {
  catalogs = [catalog('a', { auth_kind: 'basic' })]
  const { container } = await render(CatalogSettingsFixture)
  await expect.poll(() => rows(container)).toEqual(['Catalog a'])
  await clickRowAction(container, 'Edit')
  await expect.poll(() => inputByLabel(container, 'Password')).not.toBeNull()

  const password = inputByLabel(container, 'Password') as WaInput
  await password.updateComplete
  password.value = 'hunter2'
  password.dispatchEvent(new Event('input', { bubbles: true }))

  await submitForm(container)

  expect(await argsOf('opds_catalogs_update')).toMatchObject({
    id: 'a',
    auth: { kind: 'basic', password: 'hunter2' },
  })
})

test('a preset already subscribed to is badged rather than offered again', async () => {
  presets = [
    { title: 'Standard Ebooks', url: 'https://example.test/a/opds', description: 'Free books' },
    { title: 'Gutenberg', url: 'https://example.test/other/opds', description: 'More books' },
  ]
  const { container } = await render(CatalogSettingsFixture)

  await expect.poll(() => container.querySelectorAll('.presets wa-card').length).toBe(2)

  const badges = [...container.querySelectorAll('.presets wa-badge')].map((n) =>
    n.textContent?.trim(),
  )
  expect(badges).toEqual(['Subscribed'])
})
