<script lang="ts">
  interface Props {
    label: string;
    selected: boolean;
    ontoggle: () => void;
  }

  let { label, selected, ontoggle }: Props = $props();
</script>

<wa-button
  size="small"
  appearance={selected ? "filled" : "outlined"}
  onclick={ontoggle}
>
  {label}
</wa-button>
