<script lang="ts">
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'
import type { HighlightColor } from '$lib/reader/highlights'

interface Props {
  quote?: string
  showColors?: boolean
  initialText?: string
  initialColor?: HighlightColor
  saved: Array<{ text: string; color: HighlightColor | undefined }>
}

let { quote, showColors = false, initialText, initialColor, saved }: Props = $props()
</script>

<AnnotationComposer
  placeholder="Note text"
  {quote}
  {showColors}
  {initialText}
  {initialColor}
  onsave={(text, color) => {
    saved.push({ text, color })
  }}
  oncancel={() => {
    saved.push({ text: '<cancel>', color: undefined })
  }}
/>
