export interface FontMeta {
  id: string;
  family: string;
  variable: boolean;
  weights: number[];
  styles: string[];
  subsets: string[];
  defSubset: string;
  category: string;
  version: string;
}

const BASE = 'https://api.fontsource.org/v1/fonts';

export async function searchFonts(query: string): Promise<FontMeta[]> {
  const params = new URLSearchParams({ family: query });
  const res = await fetch(`${BASE}?${params}`);
  if (!res.ok) throw new Error(`Fontsource API error: ${res.status}`);
  const data: unknown = await res.json();
  if (!Array.isArray(data)) return [];
  return data as FontMeta[];
}
