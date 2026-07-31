import { describe, expect, it } from 'vitest'
import { render } from 'vitest-browser-svelte'
import SearchBar from '$lib/search/components/search-bar.svelte'

describe('SearchBar', () => {
  it('renders the search input custom element', async () => {
    const screen = await render(SearchBar, { value: '', placeholder: 'Search books…' })

    const input = screen.container.querySelector('wa-input')
    expect(input).toBeTruthy()
    expect(input?.getAttribute('placeholder')).toBe('Search books…')
  })
})
