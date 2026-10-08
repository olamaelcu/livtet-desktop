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
