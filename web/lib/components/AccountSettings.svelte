<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'svelte-sonner'
import {
  atprotoErrorMessage,
  atprotoGetProfile,
  atprotoLogout,
  atprotoStatus,
  beginAtprotoLogin,
  completeAtprotoLogin,
  refreshAtprotoSession,
  setAtprotoAvatar,
  updateAtprotoProfile,
} from '../atproto'
import { describeMoment } from '../datetime'
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
let callbackUrl = $state('')
let draftName = $state('')
let draftDescription = $state('')
let hydratedFor = $state<string | null>(null)
let showEditor = $state(false)
let showDebug = $state(false)

/** The platform proxy 404s when the account has no Bluesky avatar either. */
function hideMissingAvatar(event: Event) {
  const image = event.currentTarget
  if (image instanceof HTMLImageElement) image.style.display = 'none'
}

// Seed the editor once per account; later query refreshes must not clobber
// whatever the user is typing.
$effect(() => {
  const data = profile.data
  if (!data || hydratedFor === data.did) return
  draftName = data.display_name ?? ''
  draftDescription = data.description ?? ''
  hydratedFor = data.did
})

const signedIn = $derived(status.data ?? null)

/** Clock the token's countdown is measured against; it moves while a session shows. */
let now = $state(new Date())

/**
 * The expiry on the reader's own clock, and how far off it is.
 *
 * `undefined` for a missing or unparseable timestamp, so the panel falls back to
 * the raw value rather than printing "Invalid Date".
 */
const expiry = $derived(signedIn?.expires_at ? describeMoment(signedIn.expires_at, now) : undefined)

/**
 * Expiry the backend confirmed the server still accepts.
 *
 * A PDS can serve requests with an access token past the lifetime it advertised,
 * and a token that is still accepted must not be reported as expired: that reads
 * as "sign in again" when nothing is wrong. Refreshing the session settles it,
 * and the verdict holds until the stored expiry itself changes.
 */
let confirmedLive = $state<string | null>(null)

/**
 * Whether the stored expiry has already passed, and has not been shown to be
 * still accepted.
 *
 * An unparseable timestamp compares false, so nothing offers to refresh a value
 * that cannot be reasoned about.
 */
const expired = $derived(
  signedIn?.expires_at
    ? Date.parse(signedIn.expires_at) <= now.getTime() && signedIn.expires_at !== confirmedLive
    : false,
)

// "In 42 minutes" is wrong a minute after it is drawn, so the clock ticks for as
// long as an expiry is on screen. Thirty seconds keeps the minute reading honest
// without waking for a change that would not show.
$effect(() => {
  if (!signedIn?.expires_at) return
  now = new Date()
  const id = setInterval(() => (now = new Date()), 30_000)
  return () => clearInterval(id)
})

function invalidate() {
  queryClient.invalidateQueries({ queryKey: atprotoKeys.all })
}

/**
 * Ask the backend to spend the refresh token, and report what actually happened.
 *
 * The expiry only moves when the authorization server issued a new access token,
 * so an unchanged one means the old token was still accepted: worth saying rather
 * than claiming a refresh that did not occur.
 */
const refreshSession = createMutation(() => ({
  mutationFn: refreshAtprotoSession,
  onSuccess: (fresh) => {
    if (!fresh) {
      // The account went away between the click and the reply; nothing rotated.
      toast.error('No ATProto account is signed in')
      invalidate()
      return
    }
    const rotated = fresh?.expires_at !== signedIn?.expires_at
    // Nothing rotated means the old token is still being served, so stop calling
    // it expired; a rotation moves the expiry and the question with it.
    if (!rotated) confirmedLive = signedIn?.expires_at ?? null
    toast.success(rotated ? 'Access token refreshed' : 'The access token is still valid')
    invalidate()
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const signIn = createMutation(() => ({
  mutationFn: () => beginAtprotoLogin(handle.trim()),
  onSuccess: (start) => {
    loginUrl = start.url
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const completeLogin = createMutation(() => ({
  mutationFn: () => completeAtprotoLogin(callbackUrl.trim()),
  onSuccess: () => {
    callbackUrl = ''
    loginUrl = ''
    toast.success('Signed in to ATProto')
    invalidate()
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

const changeAvatar = createMutation(() => ({
  mutationFn: async () => {
    const selected = await openDialog({
      multiple: false,
      title: 'Choose a profile image',
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
    })
    if (typeof selected !== 'string') return null
    return setAtprotoAvatar(selected)
  },
  onSuccess: (view) => {
    if (!view) return
    queryClient.setQueryData(atprotoKeys.profile(), view)
    hydratedFor = view.did
    toast.success('Avatar updated')
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const saveProfile = createMutation(() => ({
  mutationFn: () => updateAtprotoProfile({ displayName: draftName, description: draftDescription }),
  onSuccess: (view) => {
    queryClient.setQueryData(atprotoKeys.profile(), view)
    hydratedFor = view.did
    toast.success('Profile saved to your PDS')
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const refreshProfile = createMutation(() => ({
  mutationFn: atprotoGetProfile,
  onSuccess: (data) => {
    void profile.refetch()
    toast.success(data ? 'Profile refreshed' : 'No profile record yet')
  },
  onError: (error) => toast.error(atprotoErrorMessage(error)),
}))

const scopes = $derived(signedIn?.scope.split(' ') || [])
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
              <dd>
                <ul>
                  {#each scopes as scope}
                  <li><code>{scope}</code></li>
                  {/each}
                </ul>
              </dd>
            </div>
          {/if}
          {#if signedIn.expires_at}
            <div>
              <dt>Access token expires</dt>
              <dd>
                {#if expiry}
                  <time datetime={signedIn.expires_at}>
                    {expiry.local}
                    <span class="relative">({expiry.relative})</span>
                  </time>
                {:else}
                  {signedIn.expires_at}
                {/if}
                {#if expired}
                  <div class="expired">
                    <span>The stored access token has expired.</span>
                    <ActionButton
                      size="small"
                      onclick={() => refreshSession.mutate()}
                      disabled={refreshSession.isPending}
                    >
                      {refreshSession.isPending ? 'Refreshing…' : 'Refresh session'}
                    </ActionButton>
                  </div>
                {/if}
              </dd>
            </div>
          {/if}
        </dl>
      </wa-card>

      {#if profile.isPending}
        <p class="muted">Loading your Livtet profile…</p>
      {:else if profile.data}
        <wa-card class="profile-card">
          <div class="profile-header">
            {#if profile.data.avatar_url}
              <img
                class="avatar"
                src={profile.data.avatar_url}
                alt=""
                onerror={hideMissingAvatar}
              />
            {/if}
            <div class="identity">
              <h3 class="display-name">{profile.data.display_name ?? 'No display name'}</h3>
              {#if profile.data.handle}
                <span class="handle">@{profile.data.handle}</span>
              {/if}
            </div>
          </div>
          {#if profile.data.description}
            <p class="description">{profile.data.description}</p>
          {/if}
        </wa-card>
      {:else if profile.isError}
        <p class="error">{atprotoErrorMessage(profile.error)}</p>
      {:else}
        <wa-card class="profile-card">
          <p class="muted">No profile record yet — save one below.</p>
        </wa-card>
      {/if}

      <div class="editor-trigger">
        <ActionButton onclick={() => (showEditor = !showEditor)}>
          {showEditor ? "Cancel editing" : "Edit profile"}
        </ActionButton>
      </div>
      {#if showEditor}
      <wa-card class="profile-editor">
        <h3 class="editor-heading">Edit profile</h3>
        <wa-input
          label="Display name"
          maxlength={640}
          value={draftName}
          oninput={(event) => (draftName = event.currentTarget.value)}
        ></wa-input>
        <wa-input
          label="Description"
          value={draftDescription}
          oninput={(event) => (draftDescription = event.currentTarget.value)}
        ></wa-input>
        <wa-button-group>
          <ActionButton onclick={() => saveProfile.mutate()} disabled={saveProfile.isPending}>
            Save to PDS
          </ActionButton>
          <ActionButton
            onclick={() => changeAvatar.mutate()}
            disabled={changeAvatar.isPending}
          >
            Change avatar
          </ActionButton>
        </wa-button-group>
      </wa-card>
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

    <div class="manual">
      <wa-input
        label="Callback URL"
        placeholder="net.olamaelcu.livtet:/oauth-redirect?code=…"
        value={callbackUrl}
        oninput={(event) => (callbackUrl = event.currentTarget.value)}
      ></wa-input>
      <ActionButton
        onclick={() => completeLogin.mutate()}
        disabled={completeLogin.isPending || callbackUrl.trim().length === 0}
      >
        Complete sign-in
      </ActionButton>
    </div>
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

  /* The relative reading is secondary: the wall clock is the fact. */
  .relative {
    color: var(--wa-color-text-quiet);
  }

  /* An expired token is a state to act on, not a detail, so it gets its own row
     under the expiry rather than being folded into it. */
  .expired {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    margin-block-start: var(--wa-space-2xs);
    color: var(--wa-color-warning-fill-loud, var(--wa-color-text-quiet));
  }

  .profile-card {
    --padding: var(--wa-space-l);
  }

  .profile-header {
    display: flex;
    align-items: center;
    gap: var(--wa-space-l);
  }

  .avatar {
    width: 112px;
    height: 112px;
    flex: none;
    border-radius: 50%;
    object-fit: cover;
    border: var(--wa-border-width-m) var(--wa-border-style)
      var(--wa-color-surface-border);
  }

  .identity {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    min-width: 0;
  }

  .display-name {
    margin: 0;
    font-size: var(--wa-font-size-2xl, 1.75rem);
    line-height: 1.15;
  }

  .handle {
    font-family: var(--wa-font-family-mono, ui-monospace, monospace);
    color: var(--wa-color-text-quiet);
  }

  .description {
    margin: var(--wa-space-m) 0 0;
    max-width: 60ch;
  }

  .sign-in,
  .manual {
    display: flex;
    align-items: end;
    gap: var(--wa-space-s);
  }

  .profile-editor {
    --padding: var(--wa-space-l);
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .editor-heading {
    margin: 0;
    font-size: var(--wa-font-size-l);
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
