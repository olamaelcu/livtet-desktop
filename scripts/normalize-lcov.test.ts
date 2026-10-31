import { describe, expect, test } from 'vitest'
import {
  normalizeLcov,
  realIo,
  parseSourceFiles,
  preflight,
  prepareGate,
  selectMeasurable,
} from './normalize-lcov.mjs'

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

  test('rewrites a Windows SF path to a repo-root-relative POSIX path', () => {
    // llvm-cov reports `D:\a\…\crates\…` on the Windows runner; a `/`-only
    // prefix strip left it absolute, and the preflight then refused the run.
    const lcov = 'SF:D:\\a\\repo\\crates\\a\\src\\lib.rs\nend_of_record\n'

    expect(normalizeLcov(lcov, 'D:\\a\\repo')).toBe('SF:crates/a/src/lib.rs\nend_of_record\n')
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
    // A path that is measurable and stays so; sidecar main.rs files became
    // unmeasurable once they were excluded from the reports.
    const path = 'crates/livtet-desktop/src/commands/sync.rs'

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

/** An io double whose reports satisfy every assertion. */
function fakeIo(overrides: Record<string, unknown> = {}) {
  const written = new Map<string, string>()
  const contents: Record<string, string> = {
    'rust.lcov': 'SF:/repo/crates/a/src/lib.rs\nDA:1,1\nend_of_record\n',
    'web.lcov': 'SF:web/lib/search.ts\nDA:1,1\nend_of_record\n',
  }
  return {
    written,
    repoRoot: '/repo',
    reports: [
      { name: 'rust', path: 'rust.lcov', normalizedPath: 'out/rust.norm.lcov', minRecords: 1 },
      { name: 'frontend', path: 'web.lcov', normalizedPath: 'out/web.norm.lcov', minRecords: 1 },
    ],
    readFile: (path: string) => contents[path],
    writeFile: (path: string, text: string) => void written.set(path, text),
    fileExists: () => true,
    mergeBase: () => '0f1e2d3',
    changedFiles: () => [],
    ...overrides,
  }
}

describe('prepareGate', () => {
  test('writes a normalized copy of each report', () => {
    const io = fakeIo()

    prepareGate(io)

    expect(io.written.get('out/rust.norm.lcov')).toContain('SF:crates/a/src/lib.rs')
    expect(io.written.get('out/web.norm.lcov')).toContain('SF:web/lib/search.ts')
  })

  test('returns the normalized paths for diff-cover to consume', () => {
    expect(prepareGate(fakeIo()).normalizedPaths).toEqual([
      'out/rust.norm.lcov',
      'out/web.norm.lcov',
    ])
  })

  test('refuses when a changed measurable file appears in no report', () => {
    const io = fakeIo({ changedFiles: () => ['web/lib/missing.ts'] })

    expect(() => prepareGate(io)).toThrow(/web\/lib\/missing\.ts changed and is measurable/)
  })

  test('refuses when the merge base is unresolved', () => {
    const io = fakeIo({ mergeBase: () => '' })

    expect(() => prepareGate(io)).toThrow(/merge base did not resolve/)
  })
})

describe('selectMeasurable: paths excluded from the reports', () => {
  // These are excluded from the coverage reports themselves, so they must also
  // be unmeasurable here: a file absent from every report while still matching
  // MEASURED would trip preflight assertion 5 on any change to it.
  test.each([
    ['crates/livtet-plugin-host/src/main.rs', 'binary entrypoint'],
    ['crates/livtet-sync-daemon/src/main.rs', 'binary entrypoint'],
    ['crates/livtet-desktop/src/main.rs', 'binary entrypoint'],
    ['crates/livtet-desktop/src/lib.rs', 'app bootstrap, excluded in Task 3'],
    ['scripts/stage-pdfjs.mjs', 'build staging script'],
  ])('treats %s as unmeasurable (%s)', (path) => {
    expect(selectMeasurable([path])).toEqual([])
  })

  // The exclusions must not over-reach: these share a shape with the above.
  test.each([
    ['crates/livtet-importer/src/lib.rs', 'a measured crate lib.rs'],
    ['crates/livtet-epub/src/lib.rs', 'a measured crate lib.rs'],
    ['scripts/normalize-lcov.mjs', 'the gate script, deliberately measured'],
  ])('still treats %s as measurable (%s)', (path) => {
    expect(selectMeasurable([path])).toEqual([path])
  })
})

describe('selectMeasurable: files the instrumenter never records', () => {
  test.each([
    ['crates/livtet-desktop/src/bin/generate-bindings.rs', 'excluded by the coverage:rust task'],
    ['crates/livtet-desktop/src/types.rs', 'type declarations, no executable code'],
    ['crates/livtet-desktop/src/commands/mod.rs', 'module re-exports only'],
    ['crates/livtet-audio/src/error.rs', 'thiserror derives, no executable code'],
    ['crates/livtet-epub/src/error.rs', 'thiserror derives, no executable code'],
    ['crates/livtet-mobi/src/error.rs', 'thiserror derives, no executable code'],
    ['crates/livtet-reader/src/error.rs', 'thiserror derives, no executable code'],
  ])('treats %s as unmeasurable (%s)', (path) => {
    expect(selectMeasurable([path])).toEqual([])
  })

  // error.rs is content-dependent, not name-dependent: these two carry real
  // code and are measured, so the exclusion must name files, not a pattern.
  test.each([
    ['crates/livtet-desktop/src/error.rs', 'carries executable code'],
    ['crates/livtet-pdf/src/error.rs', 'carries executable code'],
  ])('still treats %s as measurable (%s)', (path) => {
    expect(selectMeasurable([path])).toEqual([path])
  })
})

describe('preflight: assertion 6, exclusions that have gone stale', () => {
  // A file excluded here but present in a report is the dangerous direction:
  // it is being scored out of the gate while coverage data exists for it.
  test('rejects a report containing a file this script treats as unmeasurable', () => {
    const reports = [
      {
        name: 'rust',
        text: 'SF:crates/livtet-plugin-host/src/main.rs\nDA:1,1\nend_of_record\n',
        minRecords: 1,
      },
    ]

    expect(() => preflight({ ...soundInputs, reports })).toThrow(
      /crates\/livtet-plugin-host\/src\/main\.rs appears in report "rust" but is excluded here/,
    )
  })

  test('accepts a report whose files are all measurable', () => {
    expect(() => preflight(soundInputs)).not.toThrow()
  })
})

describe('realIo: the gate’s fail-closed wiring', () => {
  const UNRESOLVABLE = 'definitely-not-a-ref-9f3a7c'

  // This is the gate's kill switch. If mergeBase ever returns a resolvable ref
  // on failure instead of '', assertion 4 never fires, changedFiles goes empty,
  // assertion 5 passes trivially, and diff-cover scores an empty diff at 100% —
  // the gate would pass every PR with no test noticing.
  test('mergeBase returns an empty string when the compare branch does not resolve', () => {
    expect(realIo({ compareBranch: UNRESOLVABLE }).mergeBase()).toBe('')
  })

  test('changedFiles is empty when the merge base does not resolve', () => {
    expect(realIo({ compareBranch: UNRESOLVABLE }).changedFiles()).toEqual([])
  })

  test('mergeBase resolves a real ref to a commit sha', () => {
    expect(realIo({ compareBranch: 'HEAD' }).mergeBase()).toMatch(/^[0-9a-f]{40}$/)
  })
})
