import { describe, expect, it } from 'vitest'
import { asCommandId } from './types'

describe('asCommandId', () => {
  it('returns the same string value', () => {
    expect(asCommandId('palette.open')).toBe('palette.open')
  })

  it('brands arbitrary strings', () => {
    expect(asCommandId('custom.command')).toBe('custom.command')
  })
})
