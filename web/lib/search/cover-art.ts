// Cover-art placeholder helpers — baked locally while the real
// `livtet-covers` integration is not yet wired up. The palette
// and hash function are independent of the data source (mock
// or backend) so they live here, not in the mock-data file that
// was removed.

const PALETTE = [
  "#4a5759", // slate
  "#6b7e8a", // blue-grey
  "#8b7e6b", // warm grey
  "#7a6b8b", // mauve
  "#6b8b7e", // sage
  "#8b6b7a", // dusty rose
  "#5e6b8b", // steel blue
  "#8b6b5e", // terracotta
  "#6b8b6b", // moss
  "#8b8b6b", // olive
  "#6b6b8b", // indigo
  "#8b5e6b", // burgundy
] as const;

// FNV-1a 32-bit hash. Stable, fast, no crypto.
function hashTitle(title: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < title.length; i++) {
    h ^= title.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export function dominantColorFor(title: string): string {
  return PALETTE[hashTitle(title) % PALETTE.length];
}

// First grapheme (or first letter if the title starts with a
// non-letter). Falls back to "?" for empty titles.
export function coverLetter(title: string): string {
  const trimmed = title.trim();
  if (trimmed.length === 0) return "?";
  const first = Array.from(trimmed)[0] ?? "?";
  if (/[A-Za-zÀ-ÿ]/.test(first)) return first.toUpperCase();
  return first;
}
