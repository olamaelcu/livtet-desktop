import { describe, expect, test } from 'vitest'
import { normalizeLcov, parseSourceFiles, preflight, selectMeasurable } from './normalize-lcov.mjs'

/** A report that satisfies every assertion, for tests that vary one thing. */
const soundReport = {
  name: 'rust',
  text: 'SF:crates/a/src/lib.rs\nDA:1,1\nend_of_record\n',
  minRecords: 1,
}

/** Inputs that satisfy every assertion, for tests that vary one thing. */
const soundInputs = {
  reports: [soundReport],
  changedFiles: [],
  mergeBase: '0f1e2d3',
  fileExists: () => true,
}

describe('normalizeLcov', () => {
  test('rewrites an absolute SF path to repo-root-relative', () => {
    const lcov = 'SF:/repo/crates/a/src/lib.rs\nDA:1,1\nend_of_record\n'

    expect(normalizeLcov(lcov, '/repo')).toBe('SF:crates/a/src/lib.rs\nDA:1,1\nend_of_record\n')
  })

  // Guard, not a driven behaviour: vitest already emits relative paths, and the
  // same branch that strips the Rust prefix leaves these alone.
  test('leaves an already-relative SF path untouched', () => {
    const lcov = 'SF:web/lib/search.ts\nDA:1,1\nend_of_record\n'

    expect(normalizeLcov(lcov, '/repo')).toBe(lcov)
  })

  // A path outside the repo root must survive normalization intact so that the
  // on-disk check reports it, rather than being silently mangled into a
  // plausible-looking relative path.
  test('leaves a path outside the repo root untouched', () => {
    const lcov = 'SF:/elsewhere/vendor/thing.rs\nend_of_record\n'

    expect(normalizeLcov(lcov, '/repo')).toBe(lcov)
  })
})

describe('parseSourceFiles', () => {
  test('returns the SF path of every record', () => {
    const lcov = ['SF:a.rs', 'DA:1,1', 'end_of_record', 'SF:b.ts', 'DA:2,0', 'end_of_record', ''].join(
      '\n',
    )

    expect(parseSourceFiles(lcov)).toEqual(['a.rs', 'b.ts'])
  })
})

describe('preflight: report integrity', () => {
  test('accepts inputs that satisfy every assertion', () => {
    expect(() => preflight(soundInputs)).not.toThrow()
  })

  test('rejects a report with no records at all', () => {
    const reports = [{ ...soundReport, text: '' }]

    expect(() => preflight({ ...soundInputs, reports })).toThrow(/report "rust" contains no records/)
  })

  test('rejects a report that falls below its minimum record count', () => {
    const reports = [{ ...soundReport, minRecords: 20 }]

    expect(() => preflight({ ...soundInputs, reports })).toThrow(
      /report "rust" has 1 record\(s\), expected at least 20/,
    )
  })
})

describe('preflight: path and merge-base integrity', () => {
  test('rejects a record whose normalized path is not on disk', () => {
    const fileExists = (path: string) => path !== 'crates/a/src/lib.rs'

    expect(() => preflight({ ...soundInputs, fileExists })).toThrow(
      /report "rust" references crates\/a\/src\/lib\.rs, which is not on disk/,
    )
  })

  test('rejects an unresolved merge base', () => {
    expect(() => preflight({ ...soundInputs, mergeBase: '' })).toThrow(
      /merge base did not resolve/,
    )
  })
})

describe('selectMeasurable', () => {
  test.each([
    ['crates/livtet-epub/src/ocf.rs', 'crate source'],
    ['web/lib/library/filters.ts', 'frontend module'],
    ['web/routes/library/+page.svelte', 'route component'],
    ['scripts/normalize-lcov.mjs', 'the gate script itself'],
  ])('treats %s as measurable (%s)', (path) => {
    expect(selectMeasurable([path])).toEqual([path])
  })

  test.each([
    ['crates/livtet-epub/build.rs', 'cargo build script'],
    ['crates/livtet-mobi/src/test_support.rs', 'excluded from the Rust report'],
    ['crates/livtet-epub/tests/extraction.rs', 'compiled but never reported'],
    ['web/lib/library/filters.test.ts', 'a test file'],
    ['web/tests/browser/fixtures/DockFixture.svelte', 'a browser fixture'],
    ['e2e/library.spec.ts', 'playwright, not measured'],
    ['web/lib/bindings.ts', 'generated from Rust enums'],
    ['vitest.config.ts', 'config, outside every include glob'],
    ['README.md', 'not code'],
    ['Cargo.toml', 'not code'],
  ])('treats %s as unmeasurable (%s)', (path) => {
    expect(selectMeasurable([path])).toEqual([])
  })

  test('drops a path named in the tolerance list', () => {
    const path = 'crates/livtet-sync-daemon/src/main.rs'

    expect(selectMeasurable([path])).toEqual([path])
    expect(selectMeasurable([path], { tolerated: [path] })).toEqual([])
  })
})

describe('preflight: assertion 5, the silent-pass guard', () => {
  test('rejects a measurable changed file that appears in no report', () => {
    const changedFiles = ['web/lib/library/filters.ts']

    expect(() => preflight({ ...soundInputs, changedFiles })).toThrow(
      /web\/lib\/library\/filters\.ts changed and is measurable, but appears in no coverage report/,
    )
  })

  test('accepts a measurable changed file that does appear in a report', () => {
    const changedFiles = ['crates/a/src/lib.rs']

    expect(() => preflight({ ...soundInputs, changedFiles })).not.toThrow()
  })

  test('ignores changed files that are not measurable', () => {
    const changedFiles = ['README.md', 'Cargo.toml', 'e2e/library.spec.ts']

    expect(() => preflight({ ...soundInputs, changedFiles })).not.toThrow()
  })

  test('accepts an absent file that is named in the tolerance list', () => {
    const changedFiles = ['web/lib/library/filters.ts']

    expect(() =>
      preflight({ ...soundInputs, changedFiles, tolerated: changedFiles }),
    ).not.toThrow()
  })

  test('names every absent file, not just the first', () => {
    const changedFiles = ['web/lib/a.ts', 'web/lib/b.ts']

    expect(() => preflight({ ...soundInputs, changedFiles })).toThrow(/web\/lib\/b\.ts/)
  })
})
