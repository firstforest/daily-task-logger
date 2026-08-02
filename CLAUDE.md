# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## ドキュメント

仕様・構成の詳細は `docs/` にまとめてある。本ファイルは作業時の操作情報（コマンド・前提環境）に絞る。

- **[docs/requirements.md](docs/requirements.md)** — 要求仕様。各機能・コマンド・設定・CLI サブコマンド・ジャーナルのパス規約など。記法に対する要求（詳細は syntax.md に委譲）。
- **[docs/architecture.md](docs/architecture.md)** — 全体構成、Cargo ワークスペース構成、パースパイプライン、主要ソースファイル、ビルド構成・配布フロー。
- **[docs/syntax.md](docs/syntax.md)** — **記法の唯一の規範**。文書（パス規約・front matter・コードフェンス）、行の文法（EBNF。タスク・ログ・日付見出し・時刻メモ）、インデントと帰属、行内の記法（Wiki リンク・タグ・行末括弧）、正規化と照合、誤検出しない書き方、記法の弱さ（N-1〜N-9）、目標形の記法（配下の属性行。未実装）。
- **[docs/domain.md](docs/domain.md)** — ドメインモデル（**目標形**）。Document / Project / Task / Log / Observation に何があり、どう関係するか。エンティティ、Project の役割、関係の 4 層、同一性と参照の解決、観測値の導出。
- **[docs/design.md](docs/design.md)** — 現状の実装。設計原則（P1〜P5）、語彙と Rust 表現、走査のセマンティクス、Rust の型定義、不変条件（I-1〜I-22）、写像の事前条件、既知の弱さ（W-1〜W-8）、ドメインモデルとのずれ（G-1〜G-13）。

機能・フォーマット・設定・アーキテクチャに関する記述を更新する場合は、上記 docs を更新すること（本ファイルには重複させない）。

## Project Overview

VS Code extension ("taski") that aggregates tasks from markdown files across the workspace and displays them organized by date with clickable links back to source files. Written in TypeScript, bundled with esbuild, outputs to `dist/extension.js` as CommonJS. Also includes a Rust CLI for terminal access to the same functionality. UI strings and code comments are in Japanese.

全体構成（3 クレートの Cargo ワークスペースと WASM/CLI の関係）は [docs/architecture.md](docs/architecture.md) を参照。

## Commands

ビルド・チェック系のコマンドは `mise run` タスクとして定義されている（`mise.toml`）。Rust ツールチェインの環境解決を mise が行うため、`cargo`・`wasm-bindgen` 等を直接呼ぶ必要はない。

### mise tasks（推奨）

- `mise run build-wasm` — Rust を WASM にコンパイルし `src/pkg/` に出力（cargo + wasm-bindgen）
- `mise run build-cli` — CLI バイナリをビルド（`cli/` crate, release mode）
- `mise run test-rust` — Rust テストを実行（`cargo test`、ワークスペース全体）
- `mise run compile` — フルビルド（build-wasm + type-check + lint + esbuild）
- `mise run package` — プロダクションビルド（build-wasm + type-check + lint + minified esbuild）
- `mise run check` — TypeScript type-check + lint のみ
- `mise run release` — パッチバージョンを上げて main と tags を push

### npm scripts

- `npm run watch` — parallel watch for esbuild and tsc（WASM は再ビルドしない。先に `mise run build-wasm` を実行すること）
- `npm run check-types` — TypeScript type-check only (`tsc --noEmit`)
- `npm run lint` — ESLint on `src/`
- `npm run test` — VS Code 拡張テストを実行（VS Code インスタンスが必要。`@vscode/test-cli` + `@vscode/test-electron` を使用）

### `src/pkg/` は生成物

`src/pkg/` は `mise run build-wasm` が生成する wasm-bindgen 出力で、gitignore 対象。`src/parser.ts` がここから型ごと import しているため、**クリーンチェックアウト直後に WASM をビルドしないと `check-types` / `lint` / `compile` が解決エラーで落ちる**。`parser-core` を編集したときも、`build-wasm` を通すまで VS Code 拡張側には反映されない（CLI は `parser-core` を直接使うので即反映される）。

## テスト

### 配置ルール

ロジックを Rust に移す際は、テストも Rust 側で書くこと。

- **`parser-core/src/*.rs`** — パースロジックの単体テストは実装と同じファイルの `#[cfg(test)] mod tests` に置く。仕様の網羅はここで行う。
- **`cli/tests/<サブコマンド>_cli.rs`** — CLI の統合テスト。サブコマンドごとに 1 ファイル（`pj_cli.rs` / `list_cli.rs` / `schedule_cli.rs`）。一時ディレクトリを `$HOME` に見立てて実際のファイル・git リポジトリを作り、ビルド済みバイナリを起動して端から端まで検証する（`pj` の未反映検出が git のコミット日に依存し、構造化出力の契約はプロセスの標準出力でしか確かめられないため）。統合テストはファイルごとに別クレートなので、共通ヘルパ（`TempHome` 等）は `cli/tests/common/mod.rs` に置き各ファイルから `mod common;` で取り込む。
- **`src/test/*.test.ts`** — WASM 越しの薄い回帰テストのみ。実行に VS Code インスタンスの起動が必要なので、網羅は Rust 側に寄せる。

### 実データに対する回帰確認

パーサーの走査・帰属・照合に手を入れたときは、単体テストに加えて **`~/taski` の実データで変更前後の出力を突き合わせる**。仕様テストは書いた条件しか見ないので、「実際のノートで何件落ちるか」はこれでしか分からない。

```bash
# 変更前に取る
mise run build-cli && cp target/release/taski /tmp/taski-before
/tmp/taski-before list --format json > /tmp/base-list.json
/tmp/taski-before pj --format json --no-fetch > /tmp/base-pj.json
# schedule は日付ごとなので直近ぶんを回す
find ~/taski/journal -name '*.md' | sed 's|.*/||;s|\.md$||' | sort -r | head -40 > /tmp/dates.txt
while read -r d; do /tmp/taski-before schedule --format json --date "$d"; done < /tmp/dates.txt > /tmp/base-sched.json

# 変更後に同じものを取って diff する
```

`taski pj` は `ahead_count` / `unreported_count` が git の状態（作業中のコミット）で動くので、差分を見るときはこの 2 つを除くか、パーサー由来のフィールドだけを比べる。

### 単一テストの実行

```bash
# Rust: クレート・モジュール・テスト名で絞る（フィルタは部分一致）
mise exec -- cargo test -p parser-core pj::                       # parser-core の pj モジュールのみ
mise exec -- cargo test -p parser-core test_meta_note_is_not_meta # テスト 1 件
mise exec -- cargo test --test pj_cli unreported                  # CLI 統合テストの一部

# VS Code 拡張: mocha の grep で絞る（事前に out/ へのビルドが必要）
npm run pretest && npx vscode-test --grep "parseTasks"
```

VS Code テストは `out/` へのコンパイルが前提。`pretest` スクリプトが WASM ビルド → `out/` への tsc → `src/pkg/` を `out/pkg/` へコピー（テスト内の WASM import に必要）→ `compile` → `lint` までを行う。テストランナーは `.vscode-test.mjs` の設定に従って `out/test/**/*.test.js` を拾う。`npm run test` は `pretest` 込みで走る。

## Prerequisites

- **Rust toolchain** — `rustc` and `cargo` are managed via [mise](https://mise.jdx.dev/)（`mise.toml` で定義）。Rust 関連のビルド・テストは `mise run <task>` で実行すること。`cargo` 等を直接呼ぶ必要がある場合は `mise exec --` 経由で実行する（例: `mise exec -- cargo build`）。直接実行すると `RUSTUP_HOME` が正しく解決されない場合がある。
- **wasm-pack** — WASM パーサーのビルドに必要（`mise exec -- cargo install wasm-pack`）
- **wasm32-unknown-unknown target** — `mise exec -- rustup target add wasm32-unknown-unknown`

## CLI

CLI のビルドと配置:

- **Build**: `mise run build-cli` → binary at `target/release/taski`
- **Install**: `cargo install --path cli`

サブコマンドの一覧・仕様は [docs/requirements.md](docs/requirements.md) の「CLI 要求」を参照。

`cli/AGENTS.md` は `include_str!` でバイナリに埋め込まれ、`taski agents-md` がそのまま出力する（エージェント向けの CLI 利用ガイド）。サブコマンドやオプションを追加・変更したら、`docs/requirements.md` と併せて `cli/AGENTS.md` も更新すること。
