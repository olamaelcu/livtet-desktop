<script lang="ts">
import type { Snippet } from 'svelte'

interface Props {
  onclick: () => void
  disabled?: boolean
  variant?: 'brand'
  children: Snippet
}

let { onclick, disabled = false, variant, children }: Props = $props()
</script>

<!--
  `wa-button` is a WebAwesome custom element: its interactive semantics live in
  its shadow DOM, which Svelte's static a11y checks cannot see. Suppress those
  two rules here so every caller stays clean.
-->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<wa-button size="s" onclick={onclick} disabled={disabled} variant={variant}>
  {@render children()}
</wa-button>
