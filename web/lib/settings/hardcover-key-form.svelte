<script lang="ts">
  import { commands } from "$lib/bindings";
  import { toast } from "svelte-sonner";

  let status = $state<{ configured: boolean; last_set_at: string | null } | null>(null);
  let apiKey = $state("");
  let showKey = $state(false);
  let verifying = $state(false);
  let saving = $state(false);
  let clearing = $state(false);

  $effect(() => {
    commands.getHardcoverKey().then((r) => {
      if (r.status === "ok") status = r.data;
    });
  });

  async function onVerify() {
    if (!apiKey.trim()) return;
    verifying = true;
    try {
      const r = await commands.verifyHardcoverKey(apiKey);
      if (r.status === "ok") {
        if (r.data.valid) {
          toast.success(`Verified — connected to Hardcover as ${r.data.username ?? "unknown"}.`);
        } else {
          toast.error(r.data.error ?? "Verification failed.");
        }
      } else {
        toast.error(r.error);
      }
    } finally { verifying = false; }
  }

  async function onSave() {
    saving = true;
    try {
      const r = await commands.setHardcoverKey(apiKey);
      if (r.status === "ok") {
        status = r.data;
        apiKey = "";
        toast.success("Hardcover API key saved.");
      } else {
        toast.error(r.error);
      }
    } finally { saving = false; }
  }

  async function onClear() {
    clearing = true;
    try {
      const r = await commands.clearHardcoverKey();
      if (r.status === "ok") {
        status = r.data;
        toast.success("Hardcover API key removed.");
      } else {
        toast.error(r.error);
      }
    } finally { clearing = false; }
  }

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleString();
  }

  // TS-only cast: the runtime target is the <wa-input> custom element.
  function onKeyInput(e: Event) {
    const target = e.target as HTMLInputElement | null;
    if (target) apiKey = target.value;
  }
</script>

<section class="card">
  <header>
    <h2>Hardcover</h2>
    <p class="helper">
      Optional. Hardcover provides richer metadata (cover art, descriptions, page counts).
      Get a key at hardcover.app/account/api. It's stored in your OS keychain, not in the app.
    </p>
  </header>

  <div class="status">
    {#if status?.configured}
      <wa-badge variant="success" appearance="filled">Configured</wa-badge>
      {#if status.last_set_at}
        <span class="muted">Last set {formatDate(status.last_set_at)}</span>
      {/if}
    {:else}
      <wa-badge variant="neutral" appearance="outlined">Not configured</wa-badge>
    {/if}
  </div>

  <wa-input
    label="API key"
    type={showKey ? "text" : "password"}
    placeholder="hcv_..."
    value={apiKey}
    disabled={saving || clearing}
    oninput={onKeyInput}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <!--   wa-button is interactive (button semantics + native Enter/Space); Svelte's
           static analyzer does not recognize WA custom elements. -->
    <wa-button
      slot="end"
      size="s"
      appearance="plain"
      onclick={() => (showKey = !showKey)}
      aria-label={showKey ? "Hide key" : "Show key"}
    >
      <wa-icon name={showKey ? "eye-slash" : "eye"}></wa-icon>
    </wa-button>
  </wa-input>

  <footer class="actions">
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <wa-button
      appearance="outlined"
      onclick={onVerify}
      loading={verifying}
      disabled={!apiKey.trim() || saving || clearing}
    >
      Verify
    </wa-button>
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <wa-button
      variant="brand"
      onclick={onSave}
      loading={saving}
      disabled={!apiKey.trim() || verifying || clearing}
    >
      Save
    </wa-button>
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <wa-button
      variant="danger"
      appearance="outlined"
      onclick={onClear}
      loading={clearing}
      disabled={!status?.configured || saving || verifying}
    >
      Clear
    </wa-button>
  </footer>
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1.25rem;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: var(--wa-border-radius-m, 6px);
    background: var(--wa-color-surface-default, white);
    max-width: 32rem;
  }

  header h2 {
    margin: 0 0 0.25rem 0;
    font-size: 1.125rem;
  }

  .helper {
    margin: 0;
    font-size: 0.875rem;
    color: var(--wa-color-text-quiet, currentColor);
  }

  .status {
    display: flex;
    align-items: center;
    gap: 0.625rem;
  }

  .muted {
    font-size: 0.8125rem;
    color: var(--wa-color-text-quiet, currentColor);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
</style>