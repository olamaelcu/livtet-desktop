// Commands always-active, regardless of route. Their scope is "global".

import { goto } from "$app/navigation";
import { asCommandId, type Command } from "../types";
import { paletteState, helpState } from "../dialog-state.svelte";

export const globalCommands: readonly Command[] = [
  {
    id: asCommandId("palette.open"),
    label: "Open command palette",
    description: "Toggle the command palette.",
    category: "Window",
    icon: "terminal",
    scope: "global",
    when: () => !paletteState.open && !helpState.open,
    run: () => {
      paletteState.open = !paletteState.open;
    },
  },
  {
    id: asCommandId("help.shortcuts"),
    label: "Show shortcuts",
    description: "Toggle the keyboard shortcuts cheatsheet.",
    category: "Help",
    icon: "circle-question",
    scope: "global",
    when: () => !paletteState.open && !helpState.open,
    run: () => {
      helpState.open = !helpState.open;
    },
  },
  {
    id: asCommandId("nav.home"),
    label: "Go to home",
    description: "Navigate to the home route.",
    category: "Navigation",
    icon: "house",
    scope: "global",
    run: () => {
      void goto("/");
    },
  },
];
