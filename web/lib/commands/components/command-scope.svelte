<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { useScopeRegistry } from "../scope.svelte";
  import type { CommandScope } from "../types";

  interface Props {
    id: Exclude<CommandScope, "global">;
    children: import("svelte").Snippet;
  }

  let { id, children }: Props = $props();

  const scopes = useScopeRegistry();

  // Run on mount / cleanup on destroy instead of inside $effect. An
  // $effect that reads + writes the same `$state` creates a
  // read/write dependency and re-runs forever; here the
  // ScopeRegistry's `active` set is written by `activate()` and the
  // effect's `id`-tracked scope loops with the CommandReconciler's
  // `scopes.active` reads. Moving the call out of `$effect` into
  // onMount/onDestroy removes the loop entirely.
  onMount(() => {
    scopes.activate(id);
  });

  onDestroy(() => {
    scopes.deactivate(id);
  });
</script>

{@render children()}
