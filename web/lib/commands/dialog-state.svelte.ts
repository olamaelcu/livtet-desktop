// Lightweight reactive state for which dialogs are open. Used by the
// palette.open and help.shortcuts commands so they can predicate each
// other and toggle instead of always opening.

export const paletteState = $state({ open: false });
export const helpState = $state({ open: false });
