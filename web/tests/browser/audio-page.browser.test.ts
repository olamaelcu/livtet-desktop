import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import AudioPageFixture from './fixtures/AudioPageFixture.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

const { toast } = vi.hoisted(() => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))
vi.mock('svelte-sonner', () => ({ toast }))

function publication() {
  return {
    kind: 'Audiobook',
    edition_id: '01hzzzzzzzzzzzzzzzzzzzzzzz',
    title: 'A Scanned Audiobook',
    duration_seconds: 120,
    chapters: [
      { name: 'One', audio_start: 0, audio_end: 60 },
      { name: 'Two', audio_start: 60, audio_end: 120 },
    ],
    audio_url: 'http://127.0.0.1:9000/audio/01hz',
  }
}

beforeEach(() => {
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'reader_publication':
        return publication()
      case 'get_edition_detail':
        return {
          id: '01hzzzzzzzzzzzzzzzzzzzzzzz',
          title: 'A Scanned Audiobook',
          authors: [{ name: 'Jane Doe', role: 'aut', role_label: 'Author' }],
        }
      case 'get_edition_covers':
        return []
      case 'get_listening_progress':
        return null
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

afterEach(() => {
  for (const node of document.querySelectorAll('audio')) node.remove()
})

test('a loaded audiobook renders its title, chapters, and player', async () => {
  const { container } = await render(AudioPageFixture)

  await expect.poll(() => container.textContent).toContain('A Scanned Audiobook')
  expect(container.textContent).toContain('Jane Doe')
  expect(container.textContent).toContain('Chapters')
  expect(container.textContent).toContain('One')
  expect(container.querySelector('audio')).not.toBeNull()
})

test('an unloadable book surfaces the playback error', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'reader_publication') throw new Error('no bytes')
    return null
  })
  const { container } = await render(AudioPageFixture)

  await expect.poll(() => container.textContent).toContain("Couldn't load this book")
})

test('a non-audiobook publication reports nothing playable', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'reader_publication') {
      return {
        kind: 'Epub',
        edition_id: 'ed',
        title: 'Not audio',
        base_url: 'u',
        manifest: 'm',
        positions: 'p',
      }
    }
    return null
  })
  const { container } = await render(AudioPageFixture)

  await expect.poll(() => container.textContent).toContain('No playable audiobook found')
})
