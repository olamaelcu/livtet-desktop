<script lang="ts">
  import {
    createHotkey,
    createHotkeySequence,
  } from "@tanstack/svelte-hotkeys";
  import { useCommandRegistry } from "../registry.svelte";
  import { useScopeRegistry } from "../scope.svelte";
  import { defaultBindings } from "../defaults";
  import {
    deriveActive,
    resolvedBinding,
    type ActiveRegistration,
  } from "../hotkey-bridge.svelte";
  import type { Binding, CommandId } from "../types";

  interface Props {
    /** Reactive snapshot of the custom profile. */
    customProfile: Readonly<Record<CommandId, Binding>>;
  }

  let { customProfile }: Props = $props();

  const registry = useCommandRegistry();
  const scopes = useScopeRegistry();

  // Active registrations keyed by `${id}::${binding-json}` so a binding
  // change unmounts the old row and mounts the new one. The adapter
  // auto-unregisters the unmounted row.
  const activeRegistrations = $derived.by((): ActiveRegistration[] => {
    return [...deriveActive(registry.all(), scopes.active, customProfile)];
  });
</script>

{#each activeRegistrations as reg (`${reg.id}::${JSON.stringify(reg.binding)}`)}
  {#if Array.isArray(reg.binding)}
    {@const seq = reg.binding}
    {#key seq}
      {(() => {
        createHotkeySequence(
          () => seq,
          () => reg.command.run(),
          {
            meta: {
              name: reg.command.label,
              description: reg.command.description,
            },
            conflictBehavior: "warn",
          },
        );
        return "";
      })()}
    {/key}
  {:else}
    {@const hotkey = reg.binding}
    {#key hotkey}
      {(() => {
        createHotkey(
          () => hotkey as never,
          () => reg.command.run(),
          {
            meta: {
              name: reg.command.label,
              description: reg.command.description,
            },
            conflictBehavior: "warn",
          },
        );
        return "";
      })()}
    {/key}
  {/if}
{/each}