/**
 * Build the `--ignore-filename-regex` ERE from `coverage-ignore.txt`.
 *
 * `cargo llvm-cov` matches this against the *absolute* file paths it instruments,
 * so the separators are platform-dependent: a pattern written with `/` never
 * matches `crates\…` on Windows, which silently disables every exclusion there
 * (the preflight then refuses with "the exclusion has gone stale").
 *
 * Emitting `[\\/]` for each separator makes one file serve both platforms. This
 * lives in a script rather than an inline `grep`/`paste` pipeline because the
 * escaping has to survive shell and YAML quoting on three operating systems.
 *
 * Usage: `regex=$(node scripts/coverage-ignore-regex.mjs)`
 */

import { readFileSync } from 'node:fs'
import process from 'node:process'

/** Separator class that matches both `/` and `\` in an ERE. */
const SEP = '[\\\\/]'

/** Lines that are comments/blank are dropped; so are the Vitest globs, which are
 * not valid ERE and make llvm-cov silently drop every exclusion. */
const isRustPattern = (line) => !line.startsWith('web/') && !/\.(ts|mjs)$/.test(line)

const patterns = readFileSync('coverage-ignore.txt', 'utf8')
  .split('\n')
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith('#'))
  .filter(isRustPattern)
  .map((line) => line.replaceAll('/', SEP))

process.stdout.write(patterns.join('|'))
