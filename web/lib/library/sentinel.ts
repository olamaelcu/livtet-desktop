export interface SentinelOptions {
  /** The scroll region the observer is rooted at; viewport if not yet bound. */
  root: HTMLElement | undefined
  onvisible: () => void
}

/** Calls `onvisible` while the node is within 200px of the root's visible area. */
export function sentinel(node: HTMLElement, options: SentinelOptions) {
  let observer: IntersectionObserver | undefined
  function observe({ root, onvisible }: SentinelOptions) {
    observer?.disconnect()
    observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) onvisible()
      },
      { root: root ?? null, rootMargin: '200px' },
    )
    observer.observe(node)
  }
  observe(options)
  return {
    update: observe,
    destroy: () => observer?.disconnect(),
  }
}
