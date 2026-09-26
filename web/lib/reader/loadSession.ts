export interface LoadSession {
  begin: () => number
  isCurrent: (generation: number) => boolean
  invalidate: () => void
}

export function createLoadSession(): LoadSession {
  let generation = 0
  return {
    begin() {
      generation += 1
      return generation
    },
    isCurrent(candidate: number) {
      return candidate === generation
    },
    invalidate() {
      generation += 1
    },
  }
}
