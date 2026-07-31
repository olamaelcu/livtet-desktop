import { createTauriTest } from '@srsholmes/tauri-playwright';

const { test, expect } = createTauriTest({
  devUrl: 'http://localhost:1420',

  ipcContext: {},
  ipcMocks: {
    get_app_state: () => ({ initialized: true }),
    search_remote: () => ({ total_hits: 0, hits: [] }),
    get_digital_inventory: () => [],
    get_catalog_match: () => null,
    reindex: () => null,
    get_edition_detail: () => null,
    export_logs: () => null,
    get_cover: () => null,
    import_edition: () => null,
  },
});

export { test, expect };
