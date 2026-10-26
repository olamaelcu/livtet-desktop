<script lang="ts">
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'

interface Props {
  quote?: string
  saved: string[]
}

let { quote, saved }: Props = $props()
</script>

<AnnotationComposer
  {quote}
  onsave={(text) => {
    saved.push(text)
  }}
  oncancel={() => {
    saved.push('<cancel>')
  }}
/>
