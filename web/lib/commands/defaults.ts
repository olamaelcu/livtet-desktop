// The default command set and the default binding map. Combined into
// one module so adding a command is a single edit: drop it in the right
// definitions/* file, then add a binding row here.

import { asCommandId, type Command, type Profile, type Binding } from "./types";
import { globalCommands } from "./definitions/global";
import { searchCommands } from "./definitions/search";

export const defaultCommands: readonly Command[] = [
  ...globalCommands,
  ...searchCommands,
];

export const defaultBindings: Profile = {
  [asCommandId("palette.open")]: "Mod+K" as Binding,
  [asCommandId("help.shortcuts")]: "Shift+/" as Binding,
  [asCommandId("nav.home")]: "Mod+1" as Binding,
  [asCommandId("go.top")]: ["G", "G"] as Binding,
  [asCommandId("go.bottom")]: ["G", "Shift+G"] as Binding,
};