import { expect, test } from './fixtures';

test.describe('Library', () => {
  test.use({ mode: 'browser' });

  test('renders editions returned by the search index', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage.locator('.book-entry')).toHaveCount(2);
    await expect(tauriPage.locator('.book-entry').getByText('Dune').first()).toBeVisible();
    await expect(tauriPage.locator('.book-entry').getByText('Neuromancer').first()).toBeVisible();
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
    await expect(tauriPage.locator('.book-entry')).toHaveCount(1);
    await expect(tauriPage.locator('.book-entry').getByText('Neuromancer').first()).toBeVisible();
  });

  test('focuses the search input when / is pressed', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage.locator('.book-entry')).toHaveCount(2);
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
