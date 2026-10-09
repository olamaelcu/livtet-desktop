import '@awesome.me/webawesome/dist/components/dialog/dialog.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import { beforeAll, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import CommandPalette from '$lib/components/CommandPalette.svelte'
import { COMMANDS } from '$lib/hotkeys/commands'

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }

// The palette's input is `autofocus`, so Svelte calls focus() on it during
// mount. WebAwesome's focus() delegates into the element's shadow root, which
// Lit renders in a microtask of its own. The test renderer mounts inside
// flushSync, which drains Svelte's autofocus task before Lit has rendered, so
// focus() finds no inner input and throws. Outside flushSync the natural
// microtask order lets Lit render first, which is why the app does not see
// this. Tolerate the too-early call rather than changing the component for a
// harness artefact.
beforeAll(async () => {
  await Promise.all([
    customElements.whenDefined('wa-input'),
    customElements.whenDefined('wa-dialog'),
  ])
  const proto = customElements.get('wa-input')?.prototype as { focus(): void } | undefined
  if (!proto) throw new Error('wa-input did not register')
  const original = proto.focus
  proto.focus = function patchedFocus(this: HTMLElement) {
    try {
      original.call(this)
    } catch {
      // Shadow root not rendered yet; the real app focuses on the next task.
    }
  }
})

async function open() {
  const onrun = vi.fn()
  const onclose = vi.fn()
  const { container } = await render(CommandPalette, { props: { onrun, onclose } })
  const input = container.querySelector<WaInput>('wa-input')
  expect(input, 'the palette renders a filter input').not.toBeNull()
  await (input as WaInput).updateComplete
  return { container, input: input as WaInput, onrun, onclose }
}

function items(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.palette-item span')].map(
    (node) => node.textContent?.trim() ?? '',
  )
}

function activeLabel(container: HTMLElement): string {
  return container.querySelector('.palette-item.active span')?.textContent?.trim() ?? ''
}

async function type(input: WaInput, value: string) {
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

function press(input: WaInput, key: string) {
  input.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
}

test('lists every command before any filtering', async () => {
  const { container } = await open()

  expect(items(container)).toEqual(COMMANDS.map((command) => command.label))
})

test('filters by label, ignoring case', async () => {
  const { container, input } = await open()

  await type(input, 'LIBRARY')

  await expect.poll(() => items(container)).toEqual(['Go to Library'])
})

test('reports when nothing matches', async () => {
  const { container, input } = await open()

  await type(input, 'zzz-no-such-command')

  await expect.poll(() => items(container)).toEqual([])
  expect(container.querySelector('.palette-empty')?.textContent?.trim()).toBe(
    'No matching commands.',
  )
})

test('starts with the first result active', async () => {
  const { container } = await open()

  expect(activeLabel(container)).toBe(COMMANDS[0].label)
})

test('arrowing down moves the active result', async () => {
  const { container, input } = await open()

  press(input, 'ArrowDown')

  await expect.poll(() => activeLabel(container)).toBe(COMMANDS[1].label)
})

test('arrowing up from the first result wraps to the last', async () => {
  const { container, input } = await open()

  press(input, 'ArrowUp')

  await expect.poll(() => activeLabel(container)).toBe(COMMANDS[COMMANDS.length - 1].label)
})

// `selected` is not reset by arrowing, only by typing, so `active` has to clamp
// against a shrinking result set. Without the clamp this indexes past the end
// and nothing is highlighted.
test('clamps the active result when a filter shrinks the list', async () => {
  const { container, input } = await open()

  press(input, 'ArrowUp') // selected = last index
  await expect.poll(() => activeLabel(container)).toBe(COMMANDS[COMMANDS.length - 1].label)

  input.value = 'Go to Library'
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }))
  await type(input, 'Go to Library')

  await expect.poll(() => items(container)).toEqual(['Go to Library'])
  expect(activeLabel(container)).toBe('Go to Library')
})

test('enter runs the active command and closes', async () => {
  const { input, onrun, onclose } = await open()

  press(input, 'Enter')

  expect(onrun).toHaveBeenCalledWith(COMMANDS[0].id)
  expect(onclose).toHaveBeenCalled()
})

test('enter does nothing when no command matches', async () => {
  const { input, onrun, onclose } = await open()

  await type(input, 'zzz-no-such-command')
  press(input, 'Enter')

  expect(onrun).not.toHaveBeenCalled()
  expect(onclose).not.toHaveBeenCalled()
})

test('escape closes without running anything', async () => {
  const { input, onrun, onclose } = await open()

  press(input, 'Escape')

  expect(onclose).toHaveBeenCalled()
  expect(onrun).not.toHaveBeenCalled()
})

test('clicking a result runs that command and closes', async () => {
  const { container, onrun, onclose } = await open()
  const buttons = container.querySelectorAll<HTMLButtonElement>('.palette-item')

  buttons[2].click()

  expect(onrun).toHaveBeenCalledWith(COMMANDS[2].id)
  expect(onclose).toHaveBeenCalled()
})
