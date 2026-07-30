export const PROVIDER_LABELS: Record<string, string> = {
  google_books: "Google Books",
  hardcover: "Hardcover",
  openlibrary: "OpenLibrary",
};

export function prettyProvider(id: string): string {
  return PROVIDER_LABELS[id] ?? id;
}

export const FAILURE_TOAST = (provider: string): string =>
  `${prettyProvider(provider)} didn't respond — falling back to the next provider.`;