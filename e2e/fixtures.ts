import { createTauriTest } from '@srsholmes/tauri-playwright';

/**
 * Browser-mode IPC mocks for the current Tauri surface (see
 * `web/lib/bindings.ts`). Mock handlers are serialized into the page, so they
 * may only reference data injected through `ipcContext` — never Node-side
 * helpers or closures.
 */

type Edition = {
  work_id: string;
  edition_id: string;
  title: string;
  authors: string[];
  pub_date: string;
  snippet_text: string;
  kind: 'edition';
  has_file: boolean;
};

const EDITIONS: Edition[] = [
  {
    work_id: 'work-dune',
    edition_id: 'edition-dune',
    title: 'Dune',
    authors: ['Frank Herbert'],
    pub_date: '1965-08-01',
    snippet_text: 'A desert planet and the spice that rules it.',
    kind: 'edition',
    has_file: true,
  },
  {
    work_id: 'work-neuromancer',
    edition_id: 'edition-neuromancer',
    title: 'Neuromancer',
    authors: ['William Gibson'],
    pub_date: '1984-07-01',
    snippet_text: 'A console cowboy takes one last job.',
    kind: 'edition',
    has_file: true,
  },
  // Generated volumes push the result set past PAGE_SIZE (20) so pagination
  // behavior is exercised: 45 editions total, in pages of 20 / 20 / 5.
  ...Array.from({ length: 43 }, (_, index) => ({
    work_id: `work-codex-${index + 1}`,
    edition_id: `edition-codex-${index + 1}`,
    title: `Codex Volume ${index + 1}`,
    authors: ['Test Author'],
    pub_date: '2000-01-01',
    snippet_text: 'A generated fixture volume.',
    kind: 'edition' as const,
    has_file: true,
  })),
];

export const { test, expect } = createTauriTest({
  devUrl: 'http://localhost:1420',
  ipcContext: { EDITIONS },
  ipcMocks: {
    search_editions: (args) => {
      const needle = String(args?.query ?? '')
        .trim()
        .toLowerCase();
      const matches = EDITIONS.filter((edition) =>
        edition.title.toLowerCase().includes(needle),
      );
      const offset = Number(args?.offset ?? 0);
      const limit = Number(args?.limit ?? 20);
      return { hits: matches.slice(offset, offset + limit), total: matches.length };
    },
    search_typeahead: (args) => {
      const needle = String(args?.query ?? '')
        .trim()
        .toLowerCase();
      return EDITIONS.filter((edition) => edition.title.toLowerCase().includes(needle)).slice(
        0,
        8,
      );
    },
    sync_health: () => ({
      status: 'ok',
      version: '0.1.0',
      pid: 4242,
      uptime_secs: 12,
      server_running: false,
      port: 0,
    }),
    sync_status: () => ({
      device_id: 'desktop-test',
      server_running: false,
      host: '127.0.0.1',
      port: 45231,
      latest_version: 3,
      paired_device_count: 1,
      pending_pairing_count: 0,
      requests_served: 5,
      last_request_at: null,
    }),
    sync_requests_recent: () => [],
    sync_pairing_list: () => [],
    sync_pairing_begin: () => ({
      token: 'PAIR-TOKEN-123',
      uri: 'livtet://pair?token=PAIR-TOKEN-123',
      expires_at: '2026-01-01T00:10:00Z',
    }),
    sync_devices_list: () => [
      {
        device_id: 'device-1',
        name: 'Pixel 9',
        listen_on: '192.168.1.10:45231',
        device_type_id: 'android',
        paired_at: '2026-01-01T00:00:00Z',
        last_sync_at: null,
      },
    ],
    sync_conflicts_list: () => [],
    sync_server_start: () => true,
    sync_server_stop: () => true,
  },
});
