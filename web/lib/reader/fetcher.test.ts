import { Link, NumberRange } from '@readium/shared'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { readReaderResource } = vi.hoisted(() => ({ readReaderResource: vi.fn() }))
vi.mock('./read', () => ({ readReaderResource }))

import { ReaderFetcher } from './fetcher'

const link = new Link({ href: 'OEBPS/ch01.xhtml', type: 'application/xhtml+xml' })

describe('ReaderFetcher', () => {
  beforeEach(() => readReaderResource.mockReset())

  it('delegates spine reads to readReaderResource for its edition', async () => {
    readReaderResource.mockResolvedValueOnce('<html><body>One</body></html>')
    const fetcher = new ReaderFetcher('edition-1')
    const text = await fetcher.get(link).readAsString()
    expect(readReaderResource).toHaveBeenCalledWith('edition-1', 'OEBPS/ch01.xhtml')
    expect(text).toBe('<html><body>One</body></html>')
  })

  it('exposes the link and byte length of the fetched text', async () => {
    readReaderResource.mockResolvedValueOnce('abc')
    const resource = new ReaderFetcher('edition-1').get(link)
    expect(await resource.link()).toBe(link)
    expect(await resource.length()).toBe(3)
  })

  it('supports ranged reads over the fetched bytes', async () => {
    readReaderResource.mockResolvedValue('abcdef')
    const resource = new ReaderFetcher('edition-1').get(link)
    expect(await resource.read(new NumberRange(1, 3))).toEqual(new Uint8Array([98, 99, 100]))
    expect(await resource.read()).toEqual(new TextEncoder().encode('abcdef'))
  })

  it('reports no known links up front', () => {
    expect(new ReaderFetcher('edition-1').links()).toEqual([])
  })

  it('keeps bitmap links off the text-only IPC path', async () => {
    const image = new Link({ href: 'OEBPS/cover.jpg', type: 'image/jpeg' })
    const resource = new ReaderFetcher('edition-1').get(image)
    expect(await resource.link()).toBe(image)
    expect(await resource.read()).toBeUndefined()
    expect(await resource.length()).toBeUndefined()
    expect(readReaderResource).not.toHaveBeenCalled()
  })
})
