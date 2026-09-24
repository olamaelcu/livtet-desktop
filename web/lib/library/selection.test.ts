import { describe, expect, it } from 'vitest'
import { Selection } from './selection.svelte'

describe('Selection', () => {
  it('toggles and clears', () => {
    const s = new Selection()
    s.toggle('a')
    s.toggle('b')
    s.toggle('a')
    expect([...s.selected]).toEqual(['b'])
    expect(s.count).toBe(1)
    s.clear()
    expect(s.count).toBe(0)
  })

  it('selects a whole list', () => {
    const s = new Selection()
    s.selectAll(['a', 'b', 'c'])
    expect(s.count).toBe(3)
    expect(s.mode).toBe(true)
  })
})
