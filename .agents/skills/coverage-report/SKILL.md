---
name: coverage-report
description: Use when measuring, aggregating, or viewing code coverage across the whole repository (Rust + web). Covers cargo-llvm-cov + Vitest v8, lcov parsing, HTML report generation, and opening reports.
---

# Full-Repository Coverage Report

This repository has two independent coverage producers (ADR 38):
- **Rust workspace**: `cargo llvm-cov` → `target/llvm-cov/rust.lcov` + `target/llvm-cov/html/`
- **Web (TypeScript/Svelte + scripts/*.mjs)**: Vitest v8 provider → `coverage/lcov.info` + `coverage/index.html`

No combined generator is installed (`lcov`/`genhtml` absent); `cargo llvm-cov report` cannot ingest web lcov. Deliverable is both fresh reports opened, plus a compact summary.

## Measurement Scope (ADR 38, `coverage-ignore.txt`)

**Excluded from denominator:**
- Generated bindings (`src/bindings.rs`, `web/lib/bindings.ts`)
- Test scaffolding: `livtet-test-utils/`, `**/*.test.ts`, `web/tests/`, `livtet-audio/tests/`, `test_support.rs`
- Rust doctests, Playwright e2e (`test:e2e`), binary entrypoints (`*/src/main.rs`)
- Generated SeaORM entities (`/entities/`)
- Fake/seed scaffolding (`src/fake.rs`, `livtet-data/src/seed.rs`)
- `livtet-desktop/src/lib.rs` (tested via binary/integration), `livtet-importer/src/lib.rs` (FromLua boilerplate)
- `scripts/stage-pdfjs.mjs`, `generate-bindings.rs`
- `web/routes/**/*.ts` loaders — outside Vitest `coverage.include`, not reported

## Commands

```bash
# Rust: instrument, test, lcov, HTML (depends on coverage:rust)
mise run coverage:html

# Web: Vitest coverage over unit + browser (Playwright Chromium) + ssr projects
mise run coverage:web

# Summary from both lcov files
python3 - <<'PY'
import re, os
root = os.getcwd()
def parse(path):
    files = {}
    for block in open(path, encoding='utf-8').read().split('end_of_record'):
        m = re.search(r'^SF:(.*)$', block, re.M)
        if not m: continue
        sf = m.group(1).strip()
        if sf.startswith(root + os.sep): sf = sf[len(root) + 1:]
        lf = re.search(r'^LF:(\d+)$', block, re.M)
        lh = re.search(r'^LH:(\d+)$', block, re.M)
        if lf and lh: files[sf] = (int(lh.group(1)), int(lf.group(1)))
    return files
def pct(h, t): return f'{100*h/t:.1f}%' if t else 'n/a'
def total(files, prefix=None):
    items = {k:v for k,v in files.items() if prefix is None or k.startswith(prefix)}
    h = sum(v[0] for v in items.values()); t = sum(v[1] for v in items.values())
    return h, t, len(items)
rust = parse('target/llvm-cov/rust.lcov'); web = parse('coverage/lcov.info')
rh, rt, rn = total(rust); wh, wt, wn = total(web)
print(f'Rust workspace : {rh}/{rt} lines ({pct(rh,rt)}), {rn} files')
print(f'Web            : {wh}/{wt} lines ({pct(wh,wt)}), {wn} files')
print(f'Combined       : {rh+wh}/{rt+wt} lines ({pct(rh+wh, rt+wt)})')
print()
print('Rust by crate')
for c in sorted({k.split("/")[1] for k in rust if k.startswith("crates/")}):
    h,t,n = total(rust, f'crates/{c}/')
    print(f'  {c:28s} {pct(h,t):>6s}  {h:5d}/{t:<5d}  {n} files')
print()
print('Web by area')
for area in ('web/lib/', 'web/routes/', 'scripts/'):
    h,t,n = total(web, area)
    print(f'  {area:28s} {pct(h,t):>6s}  {h:5d}/{t:<5d}  {n} files')
print()
print('Lowest line coverage (>=10 lines, both reports)')
allf = {f'rust:{k}':v for k,v in rust.items()} | {f'web:{k}':v for k,v in web.items()}
low = [(k,*v) for k,v in allf.items() if v[1] >= 10]
for k,h,t in sorted(low, key=lambda x: x[1]/x[2])[:10]:
    print(f'  {pct(h,t):>6s}  {h:4d}/{t:<4d}  {k}')
PY
```

## Verification

```bash
# All four artifacts exist, non-empty, today's date
stat -c '%y %s %n' target/llvm-cov/rust.lcov target/llvm-cov/html/index.html coverage/lcov.info coverage/index.html

# Rust index contains "Coverage Report"; web index contains "Code coverage report for All files"
# Both xdg-open return 0; Firefox process/tab present
xdg-open "$PWD/target/llvm-cov/html/index.html"; echo "rust-exit=$?"
xdg-open "$PWD/coverage/index.html"; echo "web-exit=$?"
```

## Contingencies

- If `mise run coverage:html` fails on stale objects from deleted/renamed sources: `rm -rf target/llvm-cov-target target/llvm-cov` and rerun once.
- If Rust compile/test error: stop and report; fixing code is outside scope.
- If web coverage fails after one retry (`pnpm exec vitest run --coverage`): deliver Rust HTML, state web failure verbatim, include Rust summary only.
- If `xdg-open` fails: report absolute `file://` paths for manual opening.

## Baseline (2026-10-14 pre-existing artifacts)

- Rust: 92.2% (15066/16333 lines, 52 files)
- Web: 81.3% (1654/2035 lines, 65 files)
- Combined: 91.0% (16720/18368 lines)