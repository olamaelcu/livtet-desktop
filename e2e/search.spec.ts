import { test, expect } from './fixtures'

test.describe('Search', () => {
  test.use({ mode: 'browser' })

  test('search bar accepts input', async ({ tauriPage }) => {
    await tauriPage.goto('/search')

    const searchBar = tauriPage.locator('wa-input[type="search"]')
    await expect(searchBar).toBeVisible()

    const input = searchBar.locator('input')
    await input.fill('test query')
    expect(await input.inputValue()).toBe('test query')
  })

  test('search triggers results area', async ({ tauriPage }) => {
    await tauriPage.goto('/search')

    const searchBar = tauriPage.locator('wa-input[type="search"]')
    await searchBar.locator('input').fill('some text')

    await new Promise(r => setTimeout(r, 1000))
    const results = tauriPage.getByText(/No matches|No items|results?/)
    await expect(results.first()).toBeVisible()
  })

  test('search page renders tabs and search bar', async ({ tauriPage }) => {
    await tauriPage.goto('/search')

    await expect(tauriPage.locator('wa-input[type="search"]')).toBeVisible()
    await expect(tauriPage.getByText('Local Catalog')).toBeVisible()
    await expect(tauriPage.getByText('Remote Search')).toBeVisible()
  })
})
