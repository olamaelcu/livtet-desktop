<script lang="ts">
  import { activeTheme } from "$lib/theme/active-theme.svelte";
  import { presets } from "$lib/theme/presets";
  import HexOklch from "$lib/theme/hex-oklch.svelte";
  import FontCatalog from "$lib/fonts/font-catalog.svelte";
  import FontSlotPicker from "$lib/fonts/font-slot-picker.svelte";
  import type { OklchTriplet } from "$lib/theme/types";

  let brand = $state<OklchTriplet>({ ...activeTheme.resolved.brand });
  let neutral = $state<OklchTriplet>({ ...activeTheme.resolved.neutral });
  let scale = $state(activeTheme.settings.overrides.fontSizeScale ?? 1);

  function selectPreset(id: string) {
    activeTheme.update({ presetId: id });
    brand = { ...activeTheme.resolved.brand };
    neutral = { ...activeTheme.resolved.neutral };
  }

  function updateBrand(v: OklchTriplet) {
    brand = v;
    activeTheme.update({
      overrides: { ...activeTheme.settings.overrides, brand: v },
    });
  }

  function updateNeutral(v: OklchTriplet) {
    neutral = v;
    activeTheme.update({
      overrides: { ...activeTheme.settings.overrides, neutral: v },
    });
  }

  function updateScale(v: number) {
    scale = v;
    activeTheme.update({
      overrides: { ...activeTheme.settings.overrides, fontSizeScale: v },
    });
  }

  function onScaleInput(e: Event) {
    // TS-only cast: the runtime target is the <wa-input> custom element.
    const t = e.target as HTMLInputElement | null;
    if (t) updateScale(Number(t.value));
  }
</script>

<wa-card>
  <h2>Theme</h2>

  <wa-button-group>
    {#each presets as p}
      <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
      <!--   wa-button renders a native <button> (button semantics + Enter/Space);
           Svelte's analyzer does not recognize WA custom elements. -->
      <wa-button
        variant={activeTheme.settings.presetId === p.id ? "brand" : "neutral"}
        onclick={() => selectPreset(p.id)}
      >
        {p.name}
      </wa-button>
    {/each}
  </wa-button-group>

  <wa-button-group>
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <!--   wa-button renders a native <button> (button semantics + Enter/Space);
         Svelte's analyzer does not recognize WA custom elements. -->
    <wa-button
      variant={activeTheme.settings.mode === "auto" ? "brand" : "neutral"}
      onclick={() => activeTheme.update({ mode: "auto" })}
      >Auto</wa-button
    >
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <!--   wa-button renders a native <button> (button semantics + Enter/Space);
         Svelte's analyzer does not recognize WA custom elements. -->
    <wa-button
      variant={activeTheme.settings.mode === "light" ? "brand" : "neutral"}
      onclick={() => activeTheme.update({ mode: "light" })}
      >Light</wa-button
    >
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <!--   wa-button renders a native <button> (button semantics + Enter/Space);
         Svelte's analyzer does not recognize WA custom elements. -->
    <wa-button
      variant={activeTheme.settings.mode === "dark" ? "brand" : "neutral"}
      onclick={() => activeTheme.update({ mode: "dark" })}
      >Dark</wa-button
    >
  </wa-button-group>

  <HexOklch
    label="Brand"
    value={brand}
    onColorChange={updateBrand}
  />
  <HexOklch
    label="Neutral"
    value={neutral}
    onColorChange={updateNeutral}
  />

  <FontSlotPicker slot="body" />
  <FontSlotPicker slot="heading" />
  <FontSlotPicker slot="code" />
  <FontSlotPicker slot="longform" />

  <wa-input label="Font size scale" type="range" min="0.875" max="1.25" step="0.0625" value={scale} oninput={onScaleInput}>
    <span slot="start">
      {Math.round((scale * 100) - 100)}%
    </span>
  </wa-input>

  <FontCatalog />
</wa-card>

<style>
  wa-card {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s, 0.5rem);
  }

  wa-button-group {
    flex-wrap: wrap;
  }

  wa-input {
    width: 100%;
  }
</style>
