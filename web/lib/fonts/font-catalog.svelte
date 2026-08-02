<script lang="ts">
  import FontPicker from "./font-picker.svelte";
  import type { FontMeta } from "./fontsource-api";
  import { commands } from "$lib/bindings";
  import { toast } from "svelte-sonner";

  let downloaded = $state<string[]>([]);

  async function refreshDownloaded() {
    const r = await commands.listDownloadedFonts();
    if (r.status === "ok") {
      downloaded = r.data.map((f) => f.familyId);
    }
  }

  async function onPick(meta: FontMeta) {
    if (downloaded.includes(meta.id)) {
      toast.info(`${meta.family} already downloaded`);
      return;
    }
    const r = await commands.downloadFont(meta.id, ["latin"], ["normal"]);
    if (r.status === "ok") {
      toast.success(`${meta.family} downloaded`);
      downloaded = [...downloaded, meta.id];
    } else {
      toast.error(r.error);
    }
  }

  refreshDownloaded();
</script>

<div class="font-catalog">
  <FontPicker onpick={onPick} placeholder="Search Fontsource fonts…" />
  {#if downloaded.length > 0}
    <wa-callout variant="neutral" appearance="outlined">
      Downloaded: {downloaded.length} font(s)
    </wa-callout>
  {/if}
</div>

<style>
  .font-catalog {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }
</style>
