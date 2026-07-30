import { ulid } from "ulid";
import { listen } from "@tauri-apps/api/event";
import { commands, type SearchHitRow } from "$lib/bindings";
import { toast } from "svelte-sonner";
import { FAILURE_TOAST } from "./types";

let currentRequestId: string | null = null;
let unlistenPromise: Promise<() => void> | null = null;

export async function subscribeProviderFailures(): Promise<() => void> {
  if (unlistenPromise) return unlistenPromise;
  unlistenPromise = listen<{
    request_id: string;
    provider: "google_books" | "hardcover" | "openlibrary";
    reason: string;
  }>("provider-failure", (e) => {
    if (e.payload.request_id !== currentRequestId) return;
    toast.error(FAILURE_TOAST(e.payload.provider));
  });
  return unlistenPromise;
}

export async function runSearch(
  query: string,
  limit: number,
  onResults: (hits: SearchHitRow[]) => void,
): Promise<void> {
  if (currentRequestId !== null) {
    await commands.cancelRemoteSearch(currentRequestId);
  }
  const id = ulid();
  currentRequestId = id;

  const res = await commands.remoteSearch(query, limit, id);
  if (id !== currentRequestId) return;
  if (res.status === "ok") onResults(res.data.results);
  else onResults([]);
}