import '@awesome.me/webawesome/dist/components/dialog/dialog.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

// The dialog subscribes to daemon events on mount; outside Tauri there is no
// event bridge, so the subscription is a no-op that unsubscribes cleanly.
const { listen } = vi.hoisted(() => ({ listen: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

const { toast } = vi.hoisted(() => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}))
vi.mock('svelte-sonner', () => ({ toast }))

import IncomingOfferDialogFixture from './fixtures/IncomingOfferDialogFixture.svelte'

type Offer = {
  offer_id: string
  from_device_id: string
  from_name: string | null
  title: string
  authors: string[]
  has_cover: boolean
  received_at: string
}

/** The offers the mocked daemon reports; `sync_offers_decide` empties it. */
let offers: Offer[]
let decision: {
  offer_id: string
  accepted: boolean
  delivered: boolean
  applied: number
  conflicts: number
}

const offered: Offer = {
  offer_id: 'offer-1',
  from_device_id: 'device-abc',
  from_name: 'Wooden Otter',
  title: 'The Left Hand of Darkness',
  authors: ['Ursula K. Le Guin'],
  has_cover: true,
  received_at: '2026-11-01T09:00:00Z',
}

beforeEach(() => {
  invoke.mockReset()
  listen.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  toast.warning.mockReset()
  listen.mockImplementation(async () => () => {})

  offers = [offered]
  decision = { offer_id: 'offer-1', accepted: true, delivered: true, applied: 3, conflicts: 0 }

  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case 'sync_offers_list':
        return offers
      case 'sync_offer_cover':
        return { content_type: 'image/png', data_url: 'data:image/png;base64,AAAA' }
      case 'sync_offers_decide': {
        const accepted = args?.accepted === true
        offers = []
        return { ...decision, accepted }
      }
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

test('asks about the offered book with its cover, title, and authors', async () => {
  const { container } = await render(IncomingOfferDialogFixture)

  await expect.poll(() => container.querySelector('.title')?.textContent).toBe(offered.title)
  expect(container.querySelector('.authors')?.textContent).toContain('Ursula K. Le Guin')
  expect(container.textContent).toContain('Wooden Otter')
  await expect
    .poll(() => container.querySelector('img.cover')?.getAttribute('src'))
    .toBe('data:image/png;base64,AAAA')
})

test('accepting tells the daemon and confirms the book landed', async () => {
  const { container } = await render(IncomingOfferDialogFixture)
  await expect.poll(() => container.querySelectorAll('.actions wa-button').length).toBe(2)

  container.querySelectorAll<HTMLElement>('.actions wa-button')[0]?.click()

  await expect
    .poll(() => invoke)
    .toHaveBeenCalledWith('sync_offers_decide', {
      offerId: 'offer-1',
      accepted: true,
    })
  await expect.poll(() => toast.success).toHaveBeenCalled()
  // The offer is gone, so the prompt stops asking.
  await expect.poll(() => container.querySelector('.title')).toBeNull()
})

test('declining says so, and warns when the sender could not be told', async () => {
  decision = { ...decision, accepted: false, delivered: false, applied: 0 }
  const { container } = await render(IncomingOfferDialogFixture)
  await expect.poll(() => container.querySelectorAll('.actions wa-button').length).toBe(2)

  container.querySelectorAll<HTMLElement>('.actions wa-button')[1]?.click()

  await expect
    .poll(() => invoke)
    .toHaveBeenCalledWith('sync_offers_decide', {
      offerId: 'offer-1',
      accepted: false,
    })
  await expect.poll(() => toast.warning).toHaveBeenCalled()
})

test('says nothing when no book was offered', async () => {
  offers = []
  const { container } = await render(IncomingOfferDialogFixture)

  await expect.poll(() => invoke).toHaveBeenCalledWith('sync_offers_list')
  expect(container.querySelector('wa-dialog')).toBeNull()
})
