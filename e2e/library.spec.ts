import { expect, test } from './fixtures';

test.describe('Library', () => {
  test.use({ mode: 'browser' });

  test('renders editions returned by the search index', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage.locator('.card-wrap').getByText('Dune').first()).toBeVisible();
    await expect(tauriPage.locator('.card-wrap').getByText('Neuromancer').first()).toBeVisible();
  });

  test('fills the grid past the first page without scrolling', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect
      .poll(async () => tauriPage.locator('.card-wrap').count(), { timeout: 10_000 })
      .toBeGreaterThan(20);
  });

  test('loads every result through the load-more affordance', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    // The affordance is re-rendered as pages land, so a pointer click races the
    // detachment (the hit test then lands on <html> and times out), and it is
    // briefly absent between renders. Poll instead: click whenever it exists and
    // keep going until the grid is complete.
    await expect
      .poll(
        async () => {
          const loadMore = tauriPage.getByRole('button', { name: /load more/i }).first();
          if ((await loadMore.count()) > 0) {
            await loadMore.dispatchEvent('click').catch(() => {});
          }
          return tauriPage.locator('.card-wrap').count();
        },
        { timeout: 20_000 },
      )
      .toBe(45);
    await expect(tauriPage.locator('.result-count')).toContainText('45 results');
  });

  test('offers typeahead suggestions while typing', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await tauriPage.locator('#library-search').locator('input').fill('dune');
    await expect(tauriPage.locator('.suggestion-item')).toHaveCount(1);
    await expect(tauriPage.locator('.suggestion-item')).toContainText('Dune');
  });

  test('selecting a suggestion narrows the results', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await tauriPage.locator('#library-search').locator('input').fill('neuro');
    await tauriPage.locator('.suggestion-item').first().click();
    await expect(tauriPage.locator('.card-wrap')).toHaveCount(1);
    await expect(tauriPage.locator('.card-wrap').getByText('Neuromancer').first()).toBeVisible();
  });

  test('focuses the search input when / is pressed', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage.locator('#library-search')).toBeVisible();
    await tauriPage.keyboard.press('/');
    const focused = await tauriPage.evaluate<string>(
      `(() => {
        const active = document.activeElement;
        if (!active) return 'none';
        if (active.id === 'library-search') return 'library-search';
        const root = active.getRootNode();
        if (root instanceof ShadowRoot && root.host?.id === 'library-search') return 'library-search';
        return active.tagName;
      })()`,
    );
    expect(focused).toBe('library-search');
  });
});
