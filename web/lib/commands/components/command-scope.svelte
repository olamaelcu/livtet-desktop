<script lang="ts">
import { useScopeRegistry } from '../scope.svelte'
import type { CommandScope } from '../types'

interface Props {
  id: Exclude<CommandScope, 'global'>
  children: import('svelte').Snippet
}

let { id, children }: Props = $props()

const scopes = useScopeRegistry()

$effect(() => {
  scopes.activate(id)
  return () => scopes.deactivate(id)
})
</script>

{@render children()}
