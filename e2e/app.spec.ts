import { test, expect } from './fixtures'

test.describe('App', () => {
  test.use({ mode: 'browser' })

  test('app renders the home page', async ({ tauriPage }) => {
    await tauriPage.goto('/')
    await expect(tauriPage.getByText('Welcome!')).toBeVisible()
  })

  test('app title is correct', async ({ tauriPage }) => {
    await tauriPage.goto('/')
    await expect(tauriPage).toHaveTitle('livtet')
  })

  test('navigation to edition detail shows back button and tabs', async ({ tauriPage }) => {
    await tauriPage.goto('/catalog/test-edition-1')
    await expect(tauriPage.getByText('Back to search')).toBeVisible()
    await expect(tauriPage.getByRole('tab', { name: 'Overview' })).toBeVisible()
    await expect(tauriPage).toHaveTitle(/Edition.*livtet/)
  })
})
