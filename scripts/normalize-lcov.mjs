import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

/**
 * Rewrite lcov `SF:` records to repository-root-relative POSIX paths.
 *
 * @param {string} lcovText
 * @param {string} repoRoot
 * @returns {string}
 */
export function normalizeLcov(lcovText, repoRoot) {
  const prefix = repoRoot.endsWith('/') ? repoRoot : `${repoRoot}/`
  return lcovText.replace(/^SF:(.*)$/gm, (_match, path) =>
    path.startsWith(prefix) ? `SF:${path.slice(prefix.length)}` : `SF:${path}`,
  )
}

/**
 * Every `SF:` path in an lcov report, in file order.
 *
 * @param {string} lcovText
 * @returns {string[]}
 */
export function parseSourceFiles(lcovText) {
  return [...lcovText.matchAll(/^SF:(.*)$/gm)].map((match) => match[1].trim())
}

/** Prefix shared by every refusal, so CI logs are greppable. */
const REFUSAL = 'coverage preflight:'

/**
 * Paths the two producers report on, mirroring `coverage.include` in
 * vitest.config.ts and the crate sources cargo-llvm-cov walks.
 *
 * These patterns duplicate the coverage configuration. If one side gains a
 * directory and the other does not, assertion 5 starts failing on files that
 * can never appear — which is the loud failure, and the intended one.
 */
const MEASURED = [
  /^crates\/[^/]+\/src\/.*\.rs$/,
  /^web\/lib\/.*\.ts$/,
  /^web\/.*\.svelte$/,
  /^scripts\/.*\.mjs$/,
]

/**
 * Paths that match MEASURED yet legitimately never appear in a report.
 *
 * Every entry is a deliberate exemption with a stated reason, not a catch-all
 * regex: a tolerance list nobody can read is a tolerance list nobody audits.
 */
const NOT_MEASURED = [
  /(^|\/)build\.rs$/, // cargo build scripts are not instrumented
  /(^|\/)test_support\.rs$/, // test scaffolding, excluded by the coverage:rust task
  /^crates\/[^/]+\/tests\//, // integration tests: compiled, never reported as source
  /\.test\.ts$/, // the test files themselves
  /^web\/tests\//, // browser specs and their component fixtures
  /^e2e\//, // playwright specs, out of scope by ADR 38
  /^web\/lib\/bindings\.ts$/, // generated from Rust enums on each debug build
]

/**
 * The changed files a coverage report is expected to account for.
 *
 * @param {string[]} changedFiles paths from `git diff --name-only`
 * @param {{tolerated?: string[]}} [options] paths exempted for a stated reason
 * @returns {string[]}
 */
export function selectMeasurable(changedFiles, options = {}) {
  const tolerated = new Set(options.tolerated ?? [])

  return changedFiles.filter(
    (path) =>
      MEASURED.some((pattern) => pattern.test(path)) &&
      !NOT_MEASURED.some((pattern) => pattern.test(path)) &&
      !tolerated.has(path),
  )
}

/**
 * @typedef {object} Report
 * @property {string} name identifier used in refusal messages
 * @property {string} text normalized lcov contents
 * @property {number} minRecords floor below which the producer is assumed broken
 */

/**
 * Refuse to report a coverage result unless the lcov-to-diff join is sound.
 *
 * Throws on the first assertion that fails. A silent pass is worse than no
 * gate at all, so every check here fails closed.
 *
 * @param {object} input
 * @param {Report[]} input.reports
 * @param {string} input.mergeBase resolved merge base against the compare branch
 * @param {(path: string) => boolean} input.fileExists
 * @param {string[]} [input.changedFiles] paths from `git diff --name-only`
 * @param {string[]} [input.tolerated] measurable paths exempted for a stated reason
 * @returns {{sourceFiles: string[], measurable: string[]}}
 */
export function preflight({ reports, mergeBase, fileExists, changedFiles = [], tolerated = [] }) {
  /** @type {string[]} */
  const sourceFiles = []

  for (const report of reports) {
    const files = parseSourceFiles(report.text)

    if (files.length === 0) {
      throw new Error(
        `${REFUSAL} report "${report.name}" contains no records; its producer probably failed`,
      )
    }
    if (files.length < report.minRecords) {
      throw new Error(
        `${REFUSAL} report "${report.name}" has ${files.length} record(s), expected at least ${report.minRecords}`,
      )
    }

    for (const file of files) {
      if (!fileExists(file)) {
        throw new Error(
          `${REFUSAL} report "${report.name}" references ${file}, which is not on disk; path normalization has drifted`,
        )
      }
    }

    sourceFiles.push(...files)
  }

  if (!mergeBase) {
    throw new Error(
      `${REFUSAL} merge base did not resolve; the checkout needs fetch-depth 0 for a three-dot diff`,
    )
  }

  // Assertion 5. A changed file missing from the report contributes no lines to
  // the diff, so the gate would score it as fully covered and pass. Refusing
  // here is what turns that silent pass into a build failure.
  const measurable = selectMeasurable(changedFiles, { tolerated })
  const reported = new Set(sourceFiles)
  const absent = measurable.filter((path) => !reported.has(path))

  if (absent.length > 0) {
    throw new Error(
      `${REFUSAL} ${absent.join(', ')} changed and is measurable, but appears in no coverage report; an absent file would otherwise score as fully covered`,
    )
  }

  return { sourceFiles, measurable }
}

/**
 * The two reports the gate consumes, and where their normalized copies go.
 *
 * `minRecords` is a catastrophic-emptiness floor, not a coverage target: it
 * exists so a producer that ran and emitted nothing cannot be mistaken for a
 * repository with no code. Current counts are 54 and 64.
 */
export const REPORTS = [
  {
    name: 'rust',
    path: 'target/coverage/rust.lcov',
    normalizedPath: 'target/coverage/rust.norm.lcov',
    minRecords: 30,
  },
  {
    name: 'frontend',
    path: 'coverage/lcov.info',
    normalizedPath: 'target/coverage/web.norm.lcov',
    minRecords: 40,
  },
]

/**
 * Normalize both reports, assert the join is sound, and hand diff-cover the
 * paths to score. Throws on the first failed assertion.
 *
 * @param {object} io
 * @param {string} io.repoRoot
 * @param {typeof REPORTS} io.reports
 * @param {(path: string) => string} io.readFile
 * @param {(path: string, text: string) => void} io.writeFile
 * @param {(path: string) => boolean} io.fileExists
 * @param {() => string} io.mergeBase
 * @param {() => string[]} io.changedFiles
 * @returns {{normalizedPaths: string[], sourceFiles: string[], measurable: string[]}}
 */
export function prepareGate(io) {
  const normalized = io.reports.map((report) => {
    const text = normalizeLcov(io.readFile(report.path), io.repoRoot)
    io.writeFile(report.normalizedPath, text)
    return { ...report, text }
  })

  const { sourceFiles, measurable } = preflight({
    reports: normalized,
    mergeBase: io.mergeBase(),
    fileExists: io.fileExists,
    changedFiles: io.changedFiles(),
  })

  return {
    normalizedPaths: normalized.map((report) => report.normalizedPath),
    sourceFiles,
    measurable,
  }
}

/* ------------------------------------------------------------------ CLI -- */

/* Wiring to the real filesystem, git and argv. Excluded from coverage: there
 * is no way to exercise it that would not amount to asserting that node and
 * git work. Everything above this line is tested, and is what the five
 * assertions actually live in. */
/* v8 ignore start */

/**
 * Dependencies wired to the real filesystem and git.
 *
 * The compare branch is configurable because this repository's remote is
 * named `github`, not `origin`: CI's checkout provides `origin/main`, while
 * locally the equivalent ref is `github/main`.
 *
 * @returns {Parameters<typeof prepareGate>[0]}
 */
function realIo() {
  const compareBranch = process.env.COVERAGE_COMPARE_BRANCH ?? 'origin/main'
  const git = (/** @type {string[]} */ args) =>
    execFileSync('git', args, { encoding: 'utf8' }).trim()
  const repoRoot = git(['rev-parse', '--show-toplevel'])

  /** Empty string rather than a throw, so assertion 4 reports it. */
  const mergeBase = () => {
    try {
      // stderr is discarded: an unresolvable ref is handled below, and git's
      // own 'fatal:' line reads as an unhandled error when it is not one.
      return execFileSync('git', ['merge-base', compareBranch, 'HEAD'], {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      }).trim()
    } catch {
      return ''
    }
  }

  return {
    repoRoot,
    reports: REPORTS,
    readFile: (path) => readFileSync(path, 'utf8'),
    writeFile: (path, text) => {
      mkdirSync(dirname(path), { recursive: true })
      writeFileSync(path, text)
    },
    fileExists: (path) => existsSync(path),
    mergeBase,
    changedFiles: () => {
      const base = mergeBase()
      if (!base) return []
      return git(['diff', '--name-only', base, 'HEAD']).split('\n').filter(Boolean)
    },
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const { normalizedPaths, sourceFiles, measurable } = prepareGate(realIo())
    console.log(
      `${REFUSAL.replace(':', '')} passed: ${sourceFiles.length} source files, ` +
        `${measurable.length} measurable change(s) -> ${normalizedPaths.join(' ')}`,
    )
  } catch (/** @type {any} */ error) {
    console.error(error.message)
    process.exit(1)
  }
}
/* v8 ignore stop */
