<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import {
  atprotoErrorMessage,
  atprotoGetProfile,
  atprotoLogout,
  atprotoStatus,
  beginAtprotoLogin,
  listenToAtprotoEvents,
} from '../atproto'
import { atprotoKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const status = createQuery(() => ({
  queryKey: atprotoKeys.status(),
  queryFn: atprotoStatus,
}))

const profile = createQuery(() => ({
  queryKey: atprotoKeys.profile(),
  queryFn: atprotoGetProfile,
  enabled: status.data != null,
}))

let handle = $state('')
let loginUrl = $state('')

const signedIn = $derived(status.data ?? null)

function invalidate() {
  queryClient.invalidateQueries({ queryKey: atprotoKeys.all })
}

const signIn = createMutation(() => ({
  mutationFn: () => beginAtprotoLogin(handle.trim()),
  onSuccess: (start) => {
    loginUrl = start.url
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const signOut = createMutation(() => ({
  mutationFn: atprotoLogout,
  onSuccess: () => {
    loginUrl = ''
    toast.success('Signed out of ATProto')
    invalidate()
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const refreshProfile = createMutation(() => ({
  mutationFn: atprotoGetProfile,
  onSuccess: () => {
    void profile.refetch()
    toast.success('Profile refreshed')
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

onMount(() => {
  let unlisten: (() => void) | undefined
  let cancelled = false

  void listenToAtprotoEvents({
    onLogin: () => {
      loginUrl = ''
      toast.success('Signed in to ATProto')
      invalidate()
    },
    onError: (message) => toast.error(message),
  }).then((stop) => {
    if (cancelled) stop()
    else unlisten = stop
  })

  return () => {
    cancelled = true
    unlisten?.()
  }
})
</script>

<section class="account" aria-labelledby="account-heading">
  <h2 id="account-heading">Account</h2>

  {#if status.isPending}
    <p class="muted">Checking the ATProto session…</p>
  {:else if signedIn}
    <div class="session">
      <wa-card>
        <dl class="session-details">
          <div>
            <dt>DID</dt>
            <dd><code>{signedIn.did}</code></dd>
          </div>
          {#if signedIn.scope}
            <div>
              <dt>Scope</dt>
              <dd><code>{signedIn.scope}</code></dd>
            </div>
          {/if}
          {#if signedIn.expires_at}
            <div>
              <dt>Access token expires</dt>
              <dd>{signedIn.expires_at}</dd>
            </div>
          {/if}
        </dl>
      </wa-card>

      {#if profile.isPending}
        <p class="muted">Loading the profile from the appview…</p>
      {:else if profile.data}
        <div class="profile">
          {#if profile.data.display_name}
            <strong>{profile.data.display_name}</strong>
          {/if}
          {#if profile.data.handle}
            <span class="muted">@{profile.data.handle}</span>
          {/if}
          {#if profile.data.description}
            <p class="description">{profile.data.description}</p>
          {/if}
        </div>
      {:else if profile.isError}
        <p class="error">{atprotoErrorMessage(profile.error)}</p>
      {:else}
        <p class="muted">No profile record yet</p>
      {/if}

      <wa-button-group>
        <ActionButton
          onclick={() => refreshProfile.mutate()}
          disabled={refreshProfile.isPending}
        >
          Refresh profile
        </ActionButton>
        <ActionButton onclick={() => signOut.mutate()} disabled={signOut.isPending}>
          Sign out
        </ActionButton>
      </wa-button-group>
    </div>
  {:else}
    <p class="muted">
      Sign in with your ATProto handle or DID. The session is stored in the OS
      keyring and never leaves the Rust side.
    </p>

    <div class="sign-in">
      <wa-input
        label="ATProto handle or DID"
        placeholder="you.example"
        value={handle}
        oninput={(event) => (handle = event.currentTarget.value)}
      ></wa-input>
      <ActionButton
        onclick={() => signIn.mutate()}
        disabled={signIn.isPending || handle.trim().length === 0}
      >
        Sign in
      </ActionButton>
    </div>

    {#if signIn.isPending}
      <p class="muted">Waiting for approval in your browser…</p>
    {/if}

    {#if loginUrl}
      <div class="fallback">
        <p>If your browser did not open, open this URL:</p>
        <code class="login-url">{loginUrl}</code>
      </div>
    {/if}
  {/if}
</section>

<style>
  .account {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .session {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .session-details {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    margin: 0;
  }

  .session-details dt {
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-s);
  }

  .session-details dd {
    margin: 0;
  }

  .profile {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
  }

  .description {
    margin: 0;
  }

  .sign-in {
    display: flex;
    align-items: end;
    gap: var(--wa-space-s);
  }

  .fallback {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
  }

  .login-url {
    word-break: break-all;
    user-select: all;
  }

  .muted {
    color: var(--wa-color-text-quiet);
  }

  .error {
    color: var(--wa-color-danger-fill-loud);
  }
</style>
