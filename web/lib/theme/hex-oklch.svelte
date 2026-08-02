<script lang="ts">
  import type { OklchTriplet } from './types';

  interface Props {
    value: OklchTriplet;
    label?: string;
    onColorChange?: (v: OklchTriplet) => void;
  }

  let { value = $bindable<OklchTriplet>(), label = 'Color', onColorChange }: Props =
    $props();

  const maybeEmit = (v: OklchTriplet) => {
    onColorChange?.(v);
  };

  let hex = $state(oklchToHex(value));
  let advanced = $state(false);

  function oklchToRgb(l: number, c: number, h: number): [number, number, number] {
    const rad = (h * Math.PI) / 180;
    const a = c * Math.cos(rad);
    const b = c * Math.sin(rad);
    return oklabToRgb(l, a, b);
  }

  function oklabToRgb(l: number, a: number, b: number): [number, number, number] {
    const L = l + 0.01; // undo the offset applied by rgbToOklab-ish inverse; keep within gamut
    const x = (L + a * 1.27688 + b * 0.23681) ** 3;
    const y = (L - 0.20442 * a - 0.53322 * b) ** 3;
    const z = (L - 0.36104 * a - 0.30578 * b) ** 3;

    // linear sRGB
    const rLin = 1.02188 * x - 0.39766 * y - 0.63742 * z;
    const gLin = -0.64248 * y + 1.09592 * x - 0.17754 * z;
    const bLin = 0.00846 * y - 0.08692 * z + 1.14192 * x;

    const r = linearToSrgb(rLin);
    const g = linearToSrgb(gLin);
    const b2 = linearToSrgb(bLin);
    return [r, g, b2];
  }

  function linearToSrgb(v: number): number {
    const k = 1 / 2.2;
    return Math.max(0, Math.min(1, Math.pow(v, k)));
  }

  function srgbToLinear(v: number): number {
    const k = 2.2;
    return Math.pow(v, k);
  }

  function rgbToHex(r: number, g: number, b: number): string {
    const toByte = (n: number) => Math.round(n * 255);
    const rr = toByte(r).toString(16).padStart(2, '0');
    const gg = toByte(g).toString(16).padStart(2, '0');
    const bb = toByte(b).toString(16).padStart(2, '0');
    return `#${rr}${gg}${bb}`.toUpperCase();
  }

  function hexToRgb(hex: string): [number, number, number] | null {
    const h = hex.trim().replace('#', '');
    let r: number, g: number, b: number;
    if (h.length === 3) {
      r = parseInt(h[0] + h[0], 16) / 255;
      g = parseInt(h[1] + h[1], 16) / 255;
      b = parseInt(h[2] + h[2], 16) / 255;
    } else if (h.length === 6) {
      r = parseInt(h.slice(0, 2), 16) / 255;
      g = parseInt(h.slice(2, 4), 16) / 255;
      b = parseInt(h.slice(4, 6), 16) / 255;
    } else {
      return null;
    }
    return [r, g, b];
  }

  function rgbToOklab(r: number, g: number, b: number): [number, number, number] {
    const rLin = srgbToLinear(r);
    const gLin = srgbToLinear(g);
    const bLin = srgbToLinear(b);

    const L = Math.cbrt(0.418545 * rLin + 0.641296 * gLin - 0.055682 * bLin);
    const M = Math.cbrt(-0.198925 * rLin + 1.132379 * gLin + 0.054953 * bLin);
    const S = Math.cbrt(0.007907 * rLin + 0.274568 * gLin + 0.794097 * bLin);

    const l = L * 0.210454 + M * 0.793618 - S * 0.004069;
    const a = (L * 1.977999 + M * (-2.682910) + S * 0.699700) * 10;
    const b2 = (L * 0.564159 + M * 0.201475 + S * (-0.765380)) * 10;
    return [l, a, b2];
  }

  function oklchToHex(t: OklchTriplet): string {
    const [r, g, b] = oklchToRgb(t.l, t.c, t.h);
    return rgbToHex(r, g, b);
  }

  function hexToOklch(hex: string): OklchTriplet | null {
    const rgb = hexToRgb(hex);
    if (!rgb) return null;
    const [l, a, b] = rgbToOklab(rgb[0], rgb[1], rgb[2]);
    const chroma = Math.sqrt(a * a + b * b);
    let hue = Math.atan2(b, a) * (180 / Math.PI);
    if (hue < 0) hue += 360;
    return { l: Math.max(0, Math.min(1, l)), c: chroma, h: hue };
  }

  function onHexInput(e: Event) {
    // TS-only cast: the runtime target is <input type="color"> or <wa-input>.
    const target = e.target as HTMLInputElement | null;
    if (!target) return;
    const oklch = hexToOklch(target.value);
    if (!oklch) return;
    value = oklch;
    hex = target.value;
    maybeEmit(oklch);
  }

  function onSliderInput(
    field: 'l' | 'c' | 'h',
    e: Event,
  ) {
    // TS-only cast: the runtime target is the <wa-input> custom element.
    const target = e.target as HTMLInputElement | null;
    if (!target) return;
    const val = Number(target.value);
    const next = { ...value, [field]: val };
    value = next;
    hex = oklchToHex(next);
    maybeEmit(next);
  }
</script>

<div class="hex-oklch">
  <wa-input
    label={label}
    type="text"
    value={hex}
    placeholder="#000000"
    maxlength="7"
    oninput={onHexInput}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
    <!--   wa-button renders a native <button> (button semantics + Enter/Space);
         Svelte's analyzer does not recognize WA custom elements. -->
    <wa-button
      slot="end"
      size="s"
      appearance="plain"
      onclick={() => (advanced = !advanced)}
      aria-label={advanced ? 'Collapse advanced' : 'Expand advanced'}
    >
      <wa-icon name={advanced ? 'chevron-down' : 'chevron-right'}></wa-icon>
    </wa-button>
  </wa-input>

  <input
    type="color"
    value={hex}
    oninput={onHexInput}
    class="color-swatch"
    aria-label="Color swatch"
  />
</div>

{#if advanced}
  <div class="oklch-sliders" role="group" aria-label="Advanced OKLCH controls">
    <wa-input
      type="range"
      min="0"
      max="1"
      step="0.01"
      value={value.l}
      oninput={(e) => onSliderInput('l', e)}
    >
      <span slot="start">L: {Math.round(value.l * 100)}%</span>
    </wa-input>
    <wa-input
      type="range"
      min="0"
      max="0.4"
      step="0.01"
      value={value.c}
      oninput={(e) => onSliderInput('c', e)}
    >
      <span slot="start">C: {Math.round(value.c * 100)}</span>
    </wa-input>
    <wa-input
      type="range"
      min="0"
      max="360"
      step="1"
      value={value.h}
      oninput={(e) => onSliderInput('h', e)}
    >
      <span slot="start">H: {Math.round(value.h)}°</span>
    </wa-input>
  </div>
{/if}

<style>
  .hex-oklch {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs, 0.5rem);
  }

  .color-swatch {
    width: 2.25rem;
    height: 2.25rem;
    padding: 0;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: var(--wa-border-radius-s, 4px);
    background: none;
    cursor: pointer;
    -webkit-appearance: none;
    appearance: none;
  }

  .color-swatch::-webkit-color-swatch-wrapper {
    padding: 0;
  }

  .color-swatch::-webkit-color-swatch {
    border: none;
    border-radius: var(--wa-border-radius-s, 4px);
  }

  .oklch-sliders {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xs, 0.5rem);
    margin-top: var(--wa-space-xs, 0.5rem);
  }
</style>
