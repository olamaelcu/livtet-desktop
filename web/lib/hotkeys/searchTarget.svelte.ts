import { tick } from 'svelte'
import { goto } from '$app/navigation'
import { resolve } from '$app/paths'

let target: (() => void) | null = null

/** Registers the focus handler for `search.focus`; returns its disposer. */
export function registerSearchTarget(focus: () => void): () => void {
  target = focus
  return () => {
    if (target === focus) target = null
  }
}

/** Focuses the registered search target, going to the library first if none. */
export async function focusSearchTarget(): Promise<void> {
  if (!target) {
    await goto(resolve('/library'))
    await tick()
  }
  target?.()
}
