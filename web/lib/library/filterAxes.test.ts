import { describe, expect, it } from 'vitest'
import {
  activeFilterCount,
  normalizeFilters,
  removeAxisId,
  setAxisIds,
  setSortBy,
  setSortDirection,
} from './filterAxes'

describe('setAxisIds', () => {
  it('replaces the axis selection and normalizes order', () => {
    expect(setAxisIds({}, 'tag_ids', ['b', 'a'])).toEqual({ tag_ids: ['a', 'b'] })
  })

  it('two insertion orders produce equal output', () => {
    const first = setAxisIds({}, 'tag_ids', ['b', 'a'])
    const second = setAxisIds({}, 'tag_ids', ['a', 'b'])
    expect(first).toEqual(second)
    expect(JSON.stringify(first)).toBe(JSON.stringify(second))
  })

  it('drops the axis when the selection is emptied', () => {
    expect(setAxisIds({ tag_ids: ['a'] }, 'tag_ids', [])).toEqual({})
  })
})

describe('removeAxisId', () => {
  it('drops the id and drops the axis when it becomes empty', () => {
    expect(removeAxisId({ tag_ids: ['a', 'b'] }, 'tag_ids', 'a')).toEqual({ tag_ids: ['b'] })
    expect(removeAxisId({ tag_ids: ['a'] }, 'tag_ids', 'a')).toEqual({})
  })
})

describe('setSortBy', () => {
  it('sets the field and defaults direction to descending', () => {
    expect(setSortBy({}, 'title')).toEqual({ sort_by: 'title', sort_direction: 'desc' })
  })

  it('clearing sort_by drops sort_direction', () => {
    expect(setSortBy({ sort_by: 'title', sort_direction: 'asc' }, undefined)).toEqual({})
  })
})

describe('setSortDirection', () => {
  it('keeps the direction only while a sort field is set', () => {
    expect(setSortDirection({ sort_by: 'title' }, 'asc')).toEqual({
      sort_by: 'title',
      sort_direction: 'asc',
    })
    expect(setSortDirection({}, 'asc')).toEqual({})
  })
})

describe('normalizeFilters', () => {
  it('drops empty axes and sorts each axis', () => {
    expect(normalizeFilters({ tag_ids: ['b', 'a'], genre_ids: [] })).toEqual({
      tag_ids: ['a', 'b'],
    })
  })

  it('two insertion orders produce equal output', () => {
    const a = normalizeFilters({ tag_ids: ['b', 'a'], format_ids: ['2', '1'] })
    const b = normalizeFilters({ format_ids: ['1', '2'], tag_ids: ['a', 'b'] })
    expect(JSON.stringify(a)).toBe(JSON.stringify(b))
  })

  it('drops sort_direction when there is no sort field', () => {
    expect(normalizeFilters({ sort_direction: 'asc' })).toEqual({})
  })
})

describe('activeFilterCount', () => {
  it('counts selected ids across every axis, excluding sort', () => {
    expect(activeFilterCount({})).toBe(0)
    expect(activeFilterCount({ tag_ids: ['a', 'b'], format_ids: ['x'], sort_by: 'title' })).toBe(3)
  })
})
