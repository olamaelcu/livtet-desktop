<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import type { OpdsAuthInput, OpdsCatalog, OpdsPreset } from '../bindings'
import {
  createCatalog,
  loadCatalogs,
  loadPresets,
  opdsErrorMessage,
  removeCatalog,
  testCatalog,
  updateCatalog,
} from '../opds'
import { opdsKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const catalogs = createQuery(() => ({
  queryKey: opdsKeys.catalogs(),
  queryFn: loadCatalogs,
}))

const presets = createQuery(() => ({
  queryKey: opdsKeys.presets(),
  queryFn: loadPresets,
  staleTime: Number.POSITIVE_INFINITY,
}))

type AuthKind = 'none' | 'basic' | 'bearer'

let editingId = $state<string | null>(null)
let title = $state('')
let feedUrl = $state('')
let authKind = $state<AuthKind>('none')
let username = $state('')
let password = $state('')
let token = $state('')

const subscribedUrls = $derived(new Set((catalogs.data ?? []).map((c) => c.feed_url)))

function resetForm() {
  editingId = null
  title = ''
  feedUrl = ''
  authKind = 'none'
  username = ''
  password = ''
  token = ''
}

function startEdit(catalog: OpdsCatalog) {
  editingId = catalog.id
  title = catalog.title
  feedUrl = catalog.feed_url
  authKind = catalog.auth_kind as AuthKind
  username = ''
  password = ''
  token = ''
}

/** New credentials for create (backend rejects missing secrets). */
function createAuth(): OpdsAuthInput {
  if (authKind === 'basic') return { kind: 'basic', username, password, token: null }
  if (authKind === 'bearer') return { kind: 'bearer', username: null, password: null, token }
  return { kind: 'none', username: null, password: null, token: null }
}

/**
 * Credentials for update. `undefined` leaves the stored secret untouched;
 * switching to `none` explicitly clears it.
 */
function updateAuth(): OpdsAuthInput | undefined {
  if (authKind === 'none') return { kind: 'none', username: null, password: null, token: null }
  if (authKind === 'basic') {
    if (!username && !password) return undefined
    return { kind: 'basic', username, password, token: null }
  }
  if (!token) return undefined
  return { kind: 'bearer', username: null, password: null, token }
}

function invalidate() {
  queryClient.invalidateQueries({ queryKey: opdsKeys.catalogs() })
}

const submit = createMutation(() => ({
  mutationFn: () => {
    if (editingId) {
      return updateCatalog({ id: editingId, title, feedUrl, auth: updateAuth() })
    }
    return createCatalog({ title, feedUrl, auth: createAuth() })
  },
  onSuccess: () => {
    toast.success(editingId ? 'Catalog updated' : 'Catalog added')
    resetForm()
    invalidate()
  },
  onError: (error) => toast.error(opdsErrorMessage(error)),
}))

const subscribe = createMutation(() => ({
  mutationFn: (preset: OpdsPreset) =>
    createCatalog({
      title: preset.title,
      feedUrl: preset.url,
      auth: { kind: 'none', username: null, password: null, token: null },
    }),
  onSuccess: (_data, preset) => {
    toast.success(`Subscribed to ${preset.title}`)
    invalidate()
  },
  onError: (error) => toast.error(opdsErrorMessage(error)),
}))

const test = createMutation(() => ({
  mutationFn: (id: string) => testCatalog(id),
  onSuccess: (feed) => toast.success(`Connected: ${feed.title}`),
  onError: (error) => toast.error(opdsErrorMessage(error)),
}))

const remove = createMutation(() => ({
  mutationFn: (id: string) => removeCatalog(id),
  onSuccess: () => {
    toast.success('Catalog removed')
    invalidate()
  },
  onError: (error) => toast.error(opdsErrorMessage(error)),
}))

function setAuthKind(value: string) {
  authKind = value as AuthKind
}

function onRemove(catalog: OpdsCatalog) {
  if (confirm(`Remove "${catalog.title}"?`)) remove.mutate(catalog.id)
}
</script>

<section class="catalogs" aria-labelledby="catalogs-heading">
  <h2 id="catalogs-heading">OPDS Catalogs</h2>

  <div class="catalog-list">
    {#each catalogs.data ?? [] as catalog (catalog.id)}
      <wa-card>
        <div class="catalog-row">
          <div class="catalog-info">
            <strong>{catalog.title}</strong>
            <span class="catalog-url">{catalog.feed_url}</span>
            {#if catalog.auth_kind !== 'none'}
              <wa-badge>{catalog.auth_kind}</wa-badge>
            {/if}
          </div>
          <wa-button-group>
            <ActionButton onclick={() => test.mutate(catalog.id)} disabled={test.isPending}>
              Test
            </ActionButton>
            <ActionButton onclick={() => startEdit(catalog)}>Edit</ActionButton>
            <ActionButton onclick={() => onRemove(catalog)} disabled={remove.isPending}>
              Remove
            </ActionButton>
          </wa-button-group>
        </div>
      </wa-card>
    {:else}
      <p class="empty">No catalogs yet. Add one below or subscribe to a preset.</p>
    {/each}
  </div>

  <div class="catalog-form">
    <h3>{editingId ? 'Edit catalog' : 'Add catalog'}</h3>

    <wa-input
      label="Title"
      placeholder="My catalog"
      oninput={(event) => (title = event.currentTarget.value)}
      required
    ></wa-input>

    <wa-input
      label="Feed URL"
      placeholder="https://example.org/opds"
      oninput={(event) => (feedUrl = event.currentTarget.value)}
      required
    ></wa-input>

    <wa-select
      label="Authentication"
      value={authKind}
      onchange={(event) => setAuthKind((event.target as WaSelectElement).value as string)}
    >
      <wa-option value="none">None</wa-option>
      <wa-option value="basic">Basic</wa-option>
      <wa-option value="bearer">Bearer token</wa-option>
    </wa-select>

    {#if authKind === 'basic'}
      <wa-input
        label="Username"
        oninput={(event) => (username = event.currentTarget.value)}
      ></wa-input>
      <wa-input
        label="Password"
        type="password"
        oninput={(event) => (password = event.currentTarget.value)}
      ></wa-input>
      {#if editingId}
        <p class="hint">Leave blank to keep the stored password.</p>
      {/if}
    {:else if authKind === 'bearer'}
      <wa-input
        label="Token"
        type="password"
        oninput={(event) => (token = event.currentTarget.value)}
      ></wa-input>
      {#if editingId}
        <p class="hint">Leave blank to keep the stored token.</p>
      {/if}
    {/if}

    <div class="form-actions">
      <ActionButton
        variant="brand"
        disabled={submit.isPending}
        onclick={() => submit.mutate()}
      >
        {editingId ? 'Save changes' : 'Add catalog'}
      </ActionButton>
      {#if editingId}
        <ActionButton onclick={resetForm}>Cancel</ActionButton>
      {/if}
    </div>
  </div>

  <div class="presets">
    <h3>Built-in catalogs</h3>
    {#each presets.data ?? [] as preset (preset.url)}
      <wa-card>
        <div class="catalog-row">
          <div class="catalog-info">
            <strong>{preset.title}</strong>
            <span class="catalog-url">{preset.description}</span>
          </div>
          {#if subscribedUrls.has(preset.url)}
            <wa-badge variant="success">Subscribed</wa-badge>
          {:else}
            <ActionButton onclick={() => subscribe.mutate(preset)} disabled={subscribe.isPending}>
              Subscribe
            </ActionButton>
          {/if}
        </div>
      </wa-card>
    {/each}
  </div>
</section>

<style>
  .catalogs {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .catalogs h2,
  .catalogs h3 {
    margin: 0;
  }

  .catalog-list,
  .presets {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .catalog-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-m);
    padding: var(--wa-space-s);
  }

  .catalog-info {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    min-width: 0;
  }

  .catalog-url {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
    overflow-wrap: anywhere;
  }

  .catalog-form {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
    max-width: 32rem;
    padding: var(--wa-space-m);
    border: 0.0625rem solid var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-alt);
  }

  .form-actions {
    display: flex;
    gap: var(--wa-space-s);
  }

  .hint,
  .empty {
    margin: 0;
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
  }
</style>
