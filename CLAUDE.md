# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VS Code extension ("taski") that aggregates tasks from markdown files across the workspace and displays them organized by date with clickable links back to source files. Written in TypeScript, bundled with esbuild, outputs to `dist/extension.js` as CommonJS. UI strings and code comments are in Japanese.

## Commands

- `npm run compile` — type-check + lint + esbuild (dev mode, with sourcemaps)
- `npm run watch` — parallel watch for esbuild and tsc
- `npm run package` — type-check + lint + minified production build
- `npm run check-types` — TypeScript type-check only (`tsc --noEmit`)
- `npm run lint` — ESLint on `src/`
- `npm run test` — run VS Code extension tests (requires a VS Code instance; uses `@vscode/test-cli` + `@vscode/test-electron`)

Tests require compilation to `out/` first (`npm run compile-tests`), but `npm run test` handles this via the `pretest` script. The test runner picks up `out/test/**/*.test.js` as configured in `.vscode-test.mjs`.

## Architecture

Extension with four commands (`showToday`, `addTodayLog`, `addTomorrowLog`, `toggleTask`) in one main source file:

- **`src/extension.ts`** — contains these key parts:
  1. **`parseTasks(lines, targetDate)`** — pure function (no VS Code dependency) that parses task checkboxes and their date-prefixed log entries for a specific date. Exported for direct unit testing.
  2. **`parseTasksAllDates(lines)`** — pure function that parses all tasks and log entries across all dates. Tasks without any log entries are returned with `date: ''` and `log: ''`. Exported for direct unit testing.
  3. **`collectAllTasks()`** — scans all `.md` files in the workspace, delegates parsing to `parseTasksAllDates`, and returns a `Map<string, FileTaskGroup[]>` keyed by date string.
  4. **`buildHtml(todayStr)`** — renders the Webview HTML with tasks grouped by date: today first, then other dates in reverse chronological order, then tasks with no date.

- **`src/test/parseTasks.test.ts`** — Mocha unit tests for `parseTasks` and `parseTasksAllDates` (19 test cases covering date filtering, indentation rules, nested tasks, no-date tasks, edge cases).
- **`src/test/extension.test.ts`** — Placeholder integration test suite.

The extension has no runtime dependencies beyond the VS Code API. The `vscode` module is marked external in esbuild since VS Code provides it at runtime.

## Task Markdown Format

The extension parses this structure in `.md` files:

```markdown
- [x] Task name
    - 2026-02-01: Log entry for this date
- [ ] Another task
    - 2026-02-01: Log entry
- [ ] Task with no logs
```

Log lines must be indented deeper than their parent task line. Tasks are displayed grouped by date (today first, then newest to oldest). Tasks without any log entries appear under a "日付なし" section.

## Configuration Settings

- **`taski.includeWorkspace`** — whether to scan the current workspace for markdown files (default: `false`)
- **`taski.excludeDirectories`** — glob patterns for directories to exclude from scanning (e.g., `**/archive/**`)
- **`taski.additionalDirectories`** — absolute paths of additional directories to scan beyond the workspace

## Build Configuration

- **esbuild.js** — entry `src/extension.ts` → `dist/extension.js`, platform node, format cjs. Production builds minify; dev builds include sourcemaps.
- **tsconfig.json** — target ES2022, module Node16, strict mode enabled.
- **eslint.config.mjs** — enforces camelCase/PascalCase naming, curly braces, strict equality, no throw literals, semicolons.

## Prerequisites

- **Rust toolchain** — `rustc` and `cargo` must be installed (via [rustup](https://rustup.rs/))
- **wasm-pack** — required for building the WASM parser (`cargo install wasm-pack`)
- **wasm32-unknown-unknown target** — `rustup target add wasm32-unknown-unknown`

## WASM Parser

The pure parser functions (`parseTasks`, `parseTasksAllDates`) are implemented in Rust (`parser-wasm/src/lib.rs`) and compiled to WebAssembly via wasm-pack. The TypeScript wrapper (`src/parser.ts`) re-exports them, and `src/extension.ts` re-exports from `parser.ts` to maintain the existing public API.

- `npm run build:wasm` — compiles Rust to WASM and outputs glue code to `src/pkg/`
- The esbuild plugin copies `parser_wasm_bg.wasm` to `dist/` during bundling
