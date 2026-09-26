export const CLICK_DISAMBIGUATION_DELAY_MS = 250

interface ClickDisambiguationOptions {
  onSingle: () => void
  onDouble?: () => void
  delay?: number
}

export interface ClickDisambiguation {
  handleClick: (event?: { detail?: number }) => void
  handleDoubleClick: () => void
  dispose: () => void
}

export function createClickDisambiguation(
  options: ClickDisambiguationOptions,
): ClickDisambiguation {
  const delay = options.delay ?? CLICK_DISAMBIGUATION_DELAY_MS
  let timer: ReturnType<typeof setTimeout> | undefined

  function clear() {
    if (timer !== undefined) {
      clearTimeout(timer)
      timer = undefined
    }
  }

  return {
    handleClick(event?: { detail?: number }) {
      if (!options.onDouble || event?.detail === 0) {
        clear()
        options.onSingle()
        return
      }
      clear()
      timer = setTimeout(() => {
        timer = undefined
        options.onSingle()
      }, delay)
    },
    handleDoubleClick() {
      if (!options.onDouble) return
      clear()
      options.onDouble()
    },
    dispose() {
      clear()
    },
  }
}
