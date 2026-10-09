import '@awesome.me/webawesome/dist/components/slider/slider.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import PlayerProgressFixture from './fixtures/PlayerProgressFixture.svelte'

type WaSlider = HTMLElement & { value: number; updateComplete?: Promise<unknown> }

function slider(container: HTMLElement): WaSlider {
  const element = container.querySelector<WaSlider>('wa-slider')
  expect(element, 'the progress bar renders a slider').not.toBeNull()
  return element as WaSlider
}

function times(container: HTMLElement): [string, string] {
  return [
    container.querySelector('.current-time')?.textContent?.trim() ?? '',
    container.querySelector('.duration-time')?.textContent?.trim() ?? '',
  ]
}

test('renders position and duration through the supplied formatter', async () => {
  const { container } = await render(PlayerProgressFixture, {
    props: { position: 65, duration: 200 },
  })

  expect(times(container)).toEqual(['1:05', '3:20'])
})

test('fills the slider to the fraction played', async () => {
  const { container } = await render(PlayerProgressFixture, {
    props: { position: 50, duration: 200 },
  })
  await slider(container).updateComplete

  // Svelte sets `value` as a property on the custom element, so the attribute
  // stays unset and reading it would report 0 for any input.
  await expect.poll(() => slider(container).value).toBe(25)
})

// duration is 0 before metadata loads. Without the guard this divides by zero
// and hands the slider NaN, which renders as a jumped-to-end or blank handle.
test('reports zero progress when the duration is not yet known', async () => {
  const { container } = await render(PlayerProgressFixture, {
    props: { position: 30, duration: 0 },
  })

  await slider(container).updateComplete

  const value = slider(container).value
  expect(value).toBe(0)
  expect(Number.isNaN(value)).toBe(false)
})

test('seeking reports a position in seconds, not a percentage', async () => {
  const { container } = await render(PlayerProgressFixture, {
    props: { position: 0, duration: 200 },
  })
  const control = slider(container)
  await control.updateComplete

  control.value = 25
  control.dispatchEvent(new Event('input', { bubbles: true }))

  // 25% of a 200-second track is 50 seconds, not 25.
  await expect
    .poll(() => container.querySelector('[data-seeked]')?.getAttribute('data-seeked'))
    .toBe('50')
})
