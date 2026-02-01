# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VS Code extension ("daily-task-logger") that aggregates tasks from markdown files across the workspace and displays them organized by date with clickable links back to source files. Written in TypeScript, bundled with esbuild, outputs to `dist/extension.js` as CommonJS. UI strings and code comments are in Japanese.

## Commands

- `npm run compile` — type-check + lint + esbuild (dev mode, with sourcemaps)
- `npm run watch` — parallel watch for esbuild and tsc
- `npm run package` — type-check + lint + minified production build
- `npm run check-types` — TypeScript type-check only (`tsc --noEmit`)
- `npm run lint` — ESLint on `src/`
- `npm run test` — run VS Code extension tests (requires a VS Code instance; uses `@vscode/test-cli` + `@vscode/test-electron`)

Tests require compilation to `out/` first (`npm run compile-tests`), but `npm run test` handles this via the `pretest` script. The test runner picks up `out/test/**/*.test.js` as configured in `.vscode-test.mjs`.

## Architecture

Single-command extension (`daily-task-logger.showToday`) in one main source file:

- **`src/extension.ts`** — contains two key parts:
  1. **`parseTasks(lines, targetDate)`** — pure function (no VS Code dependency) that parses task checkboxes and their date-prefixed log entries from raw text lines. Exported for direct unit testing.
  2. **`TodaysTaskProvider`** class — implements `TextDocumentContentProvider` for the `daily-tasks://` URI scheme. Scans all `.md` files in the workspace, delegates parsing to `parseTasks`, and renders a virtual markdown document with per-file sections and line-linked task entries.

- **`src/test/parseTasks.test.ts`** — Mocha unit tests for the `parseTasks` function (12 test cases covering date filtering, indentation rules, nested tasks, edge cases).
- **`src/test/extension.test.ts`** — Placeholder integration test suite.

The extension has no runtime dependencies beyond the VS Code API. The `vscode` module is marked external in esbuild since VS Code provides it at runtime.

## Task Markdown Format

The extension parses this structure in `.md` files:

```markdown
- [x] Task name
    - 2026-02-01: Log entry for this date
- [ ] Another task
    - 2026-02-01: Log entry
```

Log lines must be indented deeper than their parent task line. Only logs matching the target date appear in the output.

## Build Configuration

- **esbuild.js** — entry `src/extension.ts` → `dist/extension.js`, platform node, format cjs. Production builds minify; dev builds include sourcemaps.
- **tsconfig.json** — target ES2022, module Node16, strict mode enabled.
- **eslint.config.mjs** — enforces camelCase/PascalCase naming, curly braces, strict equality, no throw literals, semicolons.
