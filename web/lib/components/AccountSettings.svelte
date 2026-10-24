<script lang="ts">
  import { onMount } from 'svelte';
  import { apiActions } from '../lib/api';
  import { useQueryClient } from '@tanstack/react-query';
  import { atprotoKeys } from '../query/keys';
  import { beginAtprotoLogin, completeAtprotoLogin, atprotoLogout, atprotoGetProfile, atprotoStatus, atprotoErrorMessage } from '../atproto';
  import { ActionButton } from './ActionButton';
  import toast from '../components/toast.svelte';
  import { listenToAtprotoEvents } from './atprotoEvents';

  let pending = false;
  let loginUrl = '';

  async function signIn() {
    pending = true;
    try {
      const result = await beginAtprotoLogin(input);
      loginUrl = result.url;
      onLogin();
    } catch (error) {
      toast.show('Sign in failed', atprotoErrorMessage(error));
    } finally {
      pending = false;
    }
  }

  async function completeFlow(callbackUrl: string) {
    pending = true;
    try {
      const result = await completeAtprotoLogin(callbackUrl);
      onLogin();
    } catch (error) {
      toast.show('Sign in failed', atprotoErrorMessage(error));
    } finally {
      pending = false;
    }
  }

  function signOut() {
    pending = true;
    atprotoLogout().catch((error) => toast.show('Sign out failed', atprotoErrorMessage(error))).finally(() => {
      invalidateQueries({ queryKey: atprotoKeys.all });
    });
  }

  function refreshProfile() {
    apiActions.query({ queryKey: atprotoKeys.profile, refetch: true });
  }

  onMount(() => {
    return listenToAtprotoEvents({
      onLogin: () => {
        invalidateQueries({ queryKey: atprotoKeys.all });
      },
      onError: (message: string) => {
        toast.show('ATProto error', message);
      }
    });
  });
</script>

<div class="account-settings">
  {#if status !== null && !pending}
    <div class="account-row">
      <div>
        <label>Signed in as</label>
        <code>{status.did}</code>
      </div>
      <div>
        <label>Granted scope</label>
        <code>{status.scope || '—'}</code>
      </div>
    </div>
    <div class="account-row">
      <button action="refresh" onclick={refreshProfile}>
        Refresh profile
      </button>
      <button action="logout" onclick={signOut}>
        Sign out
      </button>
    </div>
    {#if profile}
      <p class="profile">{profile.display_name ?? 'No profile record yet'}</p>
    {/if}
  {:else if pending}
    <p>Waiting for approval in your browser…</p>
  {:else}
    <wa-input label="ATProto handle or DID" bind:value={handleInput}></wa-input>
    <ActionButton label="Sign in" onclick={signIn}>open url</ActionButton>
    <code class="url-box">{loginUrl}</code>
  {/if}
</div>
