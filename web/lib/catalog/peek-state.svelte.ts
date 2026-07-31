// Module-scoped reactive state for the edition-detail peek dialog.
// The CoverGrid opens this; the <PeekDialog> mounts in the layout
// and reads/writes it.

export const peekState = $state({
  open: false,
  editionId: null as string | null,
})

export function openPeek(editionId: string): void {
  peekState.editionId = editionId
  peekState.open = true
}

export function closePeek(): void {
  peekState.open = false
}
