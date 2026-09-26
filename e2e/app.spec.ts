import { expect, test } from './fixtures';

test.describe('App shell', () => {
  test.use({ mode: 'browser' });

  test('titles the window', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage).toHaveTitle('livtet');
  });

  test('shows the splash then redirects to the library', async ({ tauriPage }) => {
    await tauriPage.goto('/');
    await expect(tauriPage.getByText(/Preparing your experience/)).toBeVisible();
    await tauriPage.waitForURL('**/library');
    await expect(tauriPage.locator('#library-search')).toBeVisible();
  });

  test('navigates from the library to settings', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await tauriPage.locator('wa-button[href="/settings"]').click();
    await tauriPage.waitForURL('**/settings');
    await expect(tauriPage.getByRole('heading', { name: 'Keyboard' })).toBeVisible();
  });

  test('opens the command palette with Mod+K', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage.locator('#library-search')).toBeVisible();
    await tauriPage.keyboard.press('Control+k');
    await expect(tauriPage.getByPlaceholder('Type a command…')).toBeVisible();
  });

  test('runs the command chosen in the palette', async ({ tauriPage }) => {
    await tauriPage.goto('/library');
    await expect(tauriPage.locator('#library-search')).toBeVisible();
    await tauriPage.keyboard.press('Control+k');
    await expect(tauriPage.getByPlaceholder('Type a command…')).toBeVisible();
    await tauriPage.getByText('Go to Settings', { exact: true }).click();
    await tauriPage.waitForURL('**/settings');
  });
});
