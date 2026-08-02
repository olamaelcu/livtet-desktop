export interface OklchTriplet {
  l: number;
  c: number;
  h: number;
}

export type FontSlot = 'body' | 'heading' | 'code' | 'longform';

export interface ActiveThemeSettings {
  mode: 'light' | 'dark' | 'auto';
  presetId: string;
  overrides: {
    brand?: OklchTriplet;
    neutral?: OklchTriplet;
    surfaceStyle?: 'default' | 'raised' | 'lowered';
    fontSlots?: Partial<Record<FontSlot, string>>;
    fontSizeScale?: number;
  };
}
