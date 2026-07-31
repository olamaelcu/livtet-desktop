<script lang="ts">
import { untrack } from 'svelte'
import { useScopeRegistry } from '../scope.svelte'
import type { CommandScope } from '../types'

interface Props {
  id: Exclude<CommandScope, 'global'>
  children: import('svelte').Snippet
}

let { id, children }: Props = $props()

const scopes = useScopeRegistry()

$effect(() => {
  untrack(() => scopes.activate(id))
  return () => untrack(() => scopes.deactivate(id))
})
</script>

{@render children()}
