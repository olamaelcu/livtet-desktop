// Stage pdf.js runtime assets into `static/pdfjs/` (ADR 0037).
//
// pdf.js loads these lazily at render time rather than through the bundler:
// character maps for CJK encodings, the standard 14 fonts when a PDF does not
// embed them, ICC profiles, and the wasm decoders (JPEG 2000 among them, which
// scanned books commonly use). Without them pdf.js does not error — it renders
// blank or garbled text on exactly those files.
//
// They are copied rather than committed so the staged bytes always match the
// installed pdfjs-dist, which is why `static/pdfjs/` is gitignored.

import { cp, mkdir, rm } from 'node:fs/promises'
import { createRequire } from 'node:module'
import path from 'node:path'
import process from 'node:process'

const require = createRequire(import.meta.url)

/** Directories pdf.js fetches at runtime, relative to the package root. */
const ASSET_DIRS = ['cmaps', 'standard_fonts', 'iccs', 'wasm']

async function main() {
  const packageRoot = path.dirname(require.resolve('pdfjs-dist/package.json'))
  const target = path.resolve(import.meta.dirname, '..', 'static', 'pdfjs')

  await rm(target, { recursive: true, force: true })
  await mkdir(target, { recursive: true })

  for (const dir of ASSET_DIRS) {
    await cp(path.join(packageRoot, dir), path.join(target, dir), {
      recursive: true,
    })
  }

  const { version } = require('pdfjs-dist/package.json')
  console.log(`staged pdf.js ${version} assets (${ASSET_DIRS.join(', ')}) into static/pdfjs`)
}

main().catch((error) => {
  console.error('failed to stage pdf.js assets:', error)
  process.exit(1)
})
