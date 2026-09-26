import type { Fetcher } from '@readium/shared'
import { type Link, type NumberRange, Resource } from '@readium/shared'
import { readReaderResource } from './read'

// Binary subresources resolve through the `reader://` custom protocol from the
// base URL Readium injects into each frame, so they must never reach the
// text-only IPC path (`reader_resource` rejects non-UTF-8 bytes).
class UnavailableResource extends Resource {
  constructor(private readonly target: Link) {
    super()
  }

  async link(): Promise<Link> {
    return this.target
  }

  async length(): Promise<number | undefined> {
    return undefined
  }

  async read(): Promise<Uint8Array | undefined> {
    return undefined
  }

  close(): void {}
}

class ReaderResource extends Resource {
  private readonly data: Promise<Uint8Array>

  constructor(
    private readonly target: Link,
    editionId: string,
  ) {
    super()
    this.data = readReaderResource(editionId, target.href).then((text) =>
      new TextEncoder().encode(text),
    )
  }

  async link(): Promise<Link> {
    return this.target
  }

  async length(): Promise<number | undefined> {
    return (await this.data).length
  }

  async read(range?: NumberRange): Promise<Uint8Array | undefined> {
    const bytes = await this.data
    if (!range) return bytes
    const start = Math.max(0, range.start)
    const end = Math.min(bytes.length, range.endInclusive + 1)
    if (start >= end) return new Uint8Array(0)
    return bytes.slice(start, end)
  }

  close(): void {}
}

export class ReaderFetcher implements Fetcher {
  constructor(private readonly editionId: string) {}

  links(): Link[] {
    return []
  }

  get(link: Link): Resource {
    if (!link.mediaType.isHTML) return new UnavailableResource(link)
    return new ReaderResource(link, this.editionId)
  }

  close(): void {}
}
