<script lang="ts">
import { toast } from 'svelte-sonner'
import { commands, type IdentifierRef } from '../bindings'
import { composeIdentifier, describeCatalogError, formatIdentifier } from './format'

interface WaInput {
  value: string
}

interface Props {
  editionId: string
  /** The entry under edit; the caller opens the dialog with one selected. */
  identifier: IdentifierRef | null
  /** Whether the dialog is open, so reopening a row re-seeds the fields. */
  open: boolean
  onClose: () => void
  /** Called once a save or a delete reached the catalog. */
  onSaved: () => void
}

let { editionId, identifier, open, onClose, onSaved }: Props = $props()

let kind = $state('')
let value = $state('')
let busy = $state(false)

const canSubmit = $derived(kind.trim().length > 0 && value.trim().length > 0 && !busy)

// The dialog is reused for every row, so the fields re-read the selected entry
// each time it opens — after a cancel as well as after a successful edit.
$effect(() => {
  if (!open || identifier === null) return
  kind = identifier.kind
  value = formatIdentifier(identifier.value)
  busy = false
})

async function submit(event: SubmitEvent) {
  event.preventDefault()
  if (!identifier || !canSubmit) return

  const nextKind = kind.trim()
  const nextValue = composeIdentifier(nextKind, value, identifier.value)
  busy = true
  const result = await commands.updateEditionIdentifier(
    editionId,
    identifier.kind,
    identifier.value,
    nextKind,
    nextValue,
  )
  busy = false
  if (result.status === 'error') {
    toast.error(describeCatalogError(result.error))
    return
  }
  toast.success('Identifier updated')
  onSaved()
  onClose()
}

async function remove() {
  if (!identifier || busy) return

  busy = true
  const result = await commands.removeEditionIdentifier(
    editionId,
    identifier.kind,
    identifier.value,
  )
  busy = false
  if (result.status === 'error') {
    toast.error(describeCatalogError(result.error))
    return
  }
  toast.success('Identifier removed')
  onSaved()
  onClose()
}
</script>

<form onsubmit={submit}>
  <div class="fields">
    <wa-input
      label="Kind"
      hint="The URN scheme, e.g. isbn, oclc, doi, or other."
      value={kind}
      oninput={(event: Event) =>
        (kind = (event.currentTarget as unknown as WaInput).value)}
    ></wa-input>
    <wa-input
      label="Value"
      hint="Saved back under the urn:kind: prefix."
      value={value}
      oninput={(event: Event) =>
        (value = (event.currentTarget as unknown as WaInput).value)}
    ></wa-input>
  </div>
  <div class="actions">
    <!-- `wa-button` keeps its semantics in shadow DOM, which Svelte's a11y
         checks cannot see; the same suppression as ActionButton. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <wa-button variant="danger" onclick={remove} disabled={busy}>
      Delete
    </wa-button>
    <wa-button-group>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <wa-button variant="text" onclick={onClose} disabled={busy}>Cancel</wa-button>
      <wa-button type="submit" variant="brand" disabled={!canSubmit} loading={busy}>
        Save
      </wa-button>
    </wa-button-group>
  </div>
</form>

<style>
.fields {
display: flex;
flex-direction: column;
gap: var(--wa-space-s);
}

.actions {
display: flex;
align-items: center;
justify-content: space-between;
gap: var(--wa-space-s);
margin-block-start: var(--wa-space-l);
}
</style>
