import type { ActiveThemeSettings, OklchTriplet } from './types';
import type { ThemePreset } from './presets';

export function oklchString(t: OklchTriplet): string {
  return `oklch(${t.l} ${t.c} ${t.h})`;
}

export function tokensForActiveTheme(
  settings: ActiveThemeSettings,
  preset: ThemePreset,
): Record<string, string> {
  const brand = settings.overrides.brand ?? preset.brand;
  const neutral = settings.overrides.neutral ?? preset.neutral;
  const surfaceStyle = settings.overrides.surfaceStyle ?? preset.surfaceStyle;
  const scale = settings.overrides.fontSizeScale ?? 1;

  const tokens: Record<string, string> = {};
  tokens['--wa-color-brand-base'] = oklchString(brand);
  tokens['--wa-color-neutral-base'] = oklchString(neutral);
  tokens['--wa-font-size-scale'] = String(scale);

  const slots: Record<string, string | undefined> = {
    body: settings.overrides.fontSlots?.body,
    heading: settings.overrides.fontSlots?.heading,
    code: settings.overrides.fontSlots?.code,
    longform: settings.overrides.fontSlots?.longform,
  };
  for (const [slot, family] of Object.entries(slots)) {
    if (family) {
      tokens[`--wa-font-family-${slot}`] = `"${family}", var(--wa-font-family-${slot})`;
    }
  }
  tokens['--wa-surface-style'] = surfaceStyle;
  return tokens;
}
