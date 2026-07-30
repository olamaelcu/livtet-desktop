<script lang="ts">
  import { attachActivate } from "$lib/a11y/attachments";

  interface Props {
    label: string;
    selected: boolean;
    ontoggle: () => void;
    id?: string;
  }

  let { label, selected, ontoggle, id }: Props = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
<!--   wa-button is interactive (button semantics + native Enter/Space); Svelte's
       static analyzer does not recognize WA custom elements. attachActivate is
       a defensive second source of Enter/Space handling. -->

<wa-button
  size="s"
  appearance={selected ? "filled" : "outlined"}
  role="button"
  tabindex="0"
  onclick={ontoggle}
  {id}
  {@attach attachActivate}
>
  {label}
</wa-button>
<wa-tooltip for={`language-${label}`} content={`Show only ${label} books`}></wa-tooltip>
