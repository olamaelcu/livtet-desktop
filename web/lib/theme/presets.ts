import type { OklchTriplet } from './types';

export type PresetId = 'default-light' | 'default-dark' | 'solarized-light' | 'solarized-dark';

export interface ThemePreset {
  id: PresetId;
  name: string;
  mode: 'light' | 'dark';
  brand: OklchTriplet;
  neutral: OklchTriplet;
  surfaceStyle: 'default' | 'raised' | 'lowered';
}

export const presets: ThemePreset[] = [
  {
    id: 'default-light',
    name: 'Default Light',
    mode: 'light',
    brand: { l: 0.55, c: 0.2, h: 250 },
    neutral: { l: 0.95, c: 0, h: 0 },
    surfaceStyle: 'default',
  },
  {
    id: 'default-dark',
    name: 'Default Dark',
    mode: 'dark',
    brand: { l: 0.6, c: 0.2, h: 250 },
    neutral: { l: 0.1, c: 0, h: 0 },
    surfaceStyle: 'default',
  },
  {
    id: 'solarized-light',
    name: 'Solarized Light',
    mode: 'light',
    brand: { l: 0.61, c: 0.13, h: 205 },
    neutral: { l: 0.93, c: 0.02, h: 210 },
    surfaceStyle: 'default',
  },
  {
    id: 'solarized-dark',
    name: 'Solarized Dark',
    mode: 'dark',
    brand: { l: 0.57, c: 0.13, h: 205 },
    neutral: { l: 0.07, c: 0.02, h: 210 },
    surfaceStyle: 'default',
  },
];
