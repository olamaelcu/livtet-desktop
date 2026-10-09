import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/button-group/button-group.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/option/option.js'
import '@awesome.me/webawesome/dist/components/select/select.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import AudioPlayerFixture from './fixtures/AudioPlayerFixture.svelte'

/** A stand-in for the media element: the player only touches these four. */
function fakeAudio(overrides: Partial<Record<string, number>> = {}) {
  return { duration: 300, currentTime: 0, volume: 1, playbackRate: 1, ...overrides }
}

function buttons(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>('wa-button-group wa-button')]
}

function reported(container: HTMLElement, key: string): string | null {
  return container.querySelector(`[data-${key}]`)?.getAttribute(`data-${key}`) ?? null
}

async function clickNth(container: HTMLElement, index: number) {
  const button = buttons(container)[index] as HTMLElement & {
    updateComplete?: Promise<unknown>
  }
  await button.updateComplete
  button.click()
}

test('skipping back moves the media element and reports the new position', async () => {
  const audio = fakeAudio()
  const { container } = await render(AudioPlayerFixture, {
    props: { audio, position: 100, duration: 300 },
  })

  await clickNth(container, 0)

  expect(audio.currentTime).toBe(70)
  await expect.poll(() => reported(container, 'seeked')).toBe('70')
})

test('skipping forward advances by the same interval', async () => {
  const audio = fakeAudio()
  const { container } = await render(AudioPlayerFixture, {
    props: { audio, position: 100, duration: 300 },
  })

  await clickNth(container, 2)

  expect(audio.currentTime).toBe(130)
})

// Skipping back near the start must clamp at zero rather than seek negative,
// which a media element either rejects or treats as an error.
test('skipping back past the start clamps to zero', async () => {
  const audio = fakeAudio()
  const { container } = await render(AudioPlayerFixture, {
    props: { audio, position: 10, duration: 300 },
  })

  await clickNth(container, 0)

  expect(audio.currentTime).toBe(0)
  await expect.poll(() => reported(container, 'seeked')).toBe('0')
})

// The element's own duration wins over the prop, so a short file cannot be
// scrubbed past its end.
test('skipping forward past the end clamps to the media duration', async () => {
  const audio = fakeAudio({ duration: 120 })
  const { container } = await render(AudioPlayerFixture, {
    props: { audio, position: 100, duration: 300 },
  })

  await clickNth(container, 2)

  expect(audio.currentTime).toBe(120)
})

test('seeking without a media element reports nothing', async () => {
  const { container } = await render(AudioPlayerFixture, {
    props: { audio: null, position: 100, duration: 300 },
  })

  await clickNth(container, 0)

  expect(reported(container, 'seeked')).toBe('')
})

test('the play control reflects the playing state', async () => {
  const idle = await render(AudioPlayerFixture, { props: { isPlaying: false } })
  expect(buttons(idle.container)[1].textContent?.trim()).toContain('Play')

  const playing = await render(AudioPlayerFixture, { props: { isPlaying: true } })
  expect(buttons(playing.container)[1].textContent?.trim()).toContain('Pause')
})

test('toggling play reports once per press', async () => {
  const { container } = await render(AudioPlayerFixture, { props: { isPlaying: false } })

  await clickNth(container, 1)

  await expect.poll(() => reported(container, 'toggles')).toBe('1')
})

test('volume is reported as a fraction and applied to the element', async () => {
  const audio = fakeAudio()
  const { container } = await render(AudioPlayerFixture, { props: { audio } })
  const range = container.querySelector<HTMLInputElement>('input[type="range"]')
  expect(range, 'the player renders a volume range').not.toBeNull()

  ;(range as HTMLInputElement).value = '40'
  ;(range as HTMLInputElement).dispatchEvent(new Event('input', { bubbles: true }))

  expect(audio.volume).toBeCloseTo(0.4)
  await expect.poll(() => reported(container, 'volume')).toBe('0.4')
})

test('the volume control starts at the element’s current volume', async () => {
  const { container } = await render(AudioPlayerFixture, {
    props: { audio: fakeAudio({ volume: 0.25 }) },
  })

  const range = container.querySelector<HTMLInputElement>('input[type="range"]')
  // Svelte sets value as a property on a native input, so the attribute is
  // never written and reading it would be null for any volume.
  expect(range?.value).toBe('25')
})
