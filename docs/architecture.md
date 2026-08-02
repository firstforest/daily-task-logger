# taski アーキテクチャ

本ドキュメントは taski の構成・設計方針をまとめたものである。要求そのものは [requirements.md](requirements.md) を、ドメインモデル（目標形）は [domain.md](domain.md) を、各クレートが持つ型・不変条件は [design.md](design.md) を参照。

## 全体構成

taski は VS Code 拡張と Rust CLI の 2 つのフロントエンドを持ち、両者がパース処理を共有する構成である。

```
                     ┌─────────────────────────┐
                     │   parser-core (Rust)     │
                     │  すべてのパースロジック    │
                     └───────────┬─────────────┘
              ┌──────────────────┴──────────────────┐
              │                                      │
   ┌──────────▼───────────┐              ┌───────────▼──────────┐
   │  parser-wasm          │              │  cli (taski)          │
   │  wasm-bindgen ラッパ   │              │  parser-core 直接利用  │
   └──────────┬───────────┘              └──────────────────────┘
              │ src/pkg/ (生成物)
   ┌──────────▼───────────┐
   │  VS Code 拡張 (TS)    │
   │  src/parser.ts 経由   │
   └──────────────────────┘
```

## Cargo ワークスペース構成

3 つのクレートで構成される。

- **`parser-core`** — 全パースロジックとデータ型を持つ共有 Rust ライブラリ。`parser-wasm` と `cli` の両方から利用される唯一の実装。
- **`parser-wasm`** — `wasm-bindgen` 経由で `parser-core` の関数を TypeScript に公開する薄い WASM ラッパー。
- **`cli`** — `parser-core` を直接利用するスタンドアロン CLI バイナリ（`taski`）。`clap` で構築。

## 設計方針

- パース処理（タスク抽出・ツリー構築・スケジュール構築・タグ抽出・Wiki リンク解決）はすべて Rust (`parser-core`) に集約し、唯一の実装とする。ロジックの重複を避け、VS Code 拡張と CLI で結果を一致させる。
- ロジックを Rust に移す際は、テストも Rust 側で記述する。
- VS Code 拡張は `parser-core` を WASM 経由（`parser-wasm` → `src/pkg/` → `src/parser.ts`）で利用し、CLI は `parser-core` を直接利用する。
- 拡張本体は VS Code API 以外のランタイム依存を持たない。`vscode` モジュールは esbuild で external 指定。

## パースパイプライン

各関数の型・事前条件・出力の不変条件は [design.md](design.md) にまとめてある。ここでは並びだけを示す。

`parser-core/src/lib.rs` の主要な公開関数:

- `parse_tasks` — 指定日付のタスクを抽出
- `parse_tasks_all_dates` — 全日付のタスクを抽出
- `build_tree_data` — 日付 → ファイル → タスク → ログ の階層ツリーを構築
- `build_schedule_data` — スケジュール（時刻付き項目）を構築

補助ロジック: front matter 解析、`#tag` 抽出 (`extract_tags`)、ファイル単位の自動タグ (`extract_file_tags`)、Wiki リンク正規化・解決 (`parser-core/src/wiki_link.rs`)、PJ ノートの日報形式の解析 (`parser-core/src/pj.rs`)。

`parser-core/src/pj.rs` は PJ ノート（front matter に `project:` を持つ `note/*.md`）を扱う。`## 次の予定` / `## ログ` / `## オープンタスク` のセクション分割、Next Action の抽出、判断メタデータ（`（45分・重・@PC）`）の分離、health 判定までを担う。git・ファイルシステムに依存する集約（note の最終更新日、journal の言及、`repo:` の未反映コミット）は `cli/src/pj.rs` 側の責務で、CLI の `pj` サブコマンドから使う。

## VS Code 拡張の主要ソース

- **`src/extension.ts`** — アクティベーション、コマンド登録、パーサー関数の再エクスポート。Markdown スラッシュコマンド（`/today` `/tomorrow` `/now`）の `CompletionItemProvider` を提供。日付はすべてローカルタイムゾーン。
- **`src/taskTreeProvider.ts`** — 日付別 TreeView。WASM パーサーで階層を構築。今日は全タスク（完了・見送り含む）を進捗カウンタ付きで、他の日付は未完了タスクのみ表示（完了・見送りは非表示）。見送り（`- [-]`）は今日ビューに表示するがカウント母数に含めない。タスク状態は `TaskStatus`（`incomplete` / `completed` / `cancelled`）として `parser-core` から伝播する。
- **`src/tagTreeProvider.ts`** — タグ別 TreeView。`tagUtils.ts` で `#tag` を抽出。
- **`src/fileScanner.ts`** — Markdown ファイル探索（`findAllMarkdownUris`）。`$HOME/taski`・ワークスペース・開いているドキュメント・追加ディレクトリをスキャン。
- **`src/schedulePanel.ts`** — 15 分刻みのタイムグリッドを持つ WebviewPanel。計画と実績を対比表示。
- **`src/gitSync.ts`** — `$HOME/taski` の Git 自動同期。保存時のデバウンス同期も担う。
- **`src/taskAlertManager.ts`** — タスク開始時刻のアラート通知管理。
- **`src/wikiLinkProviders.ts` / `src/wikiLinkCompletion.ts`** — Wiki リンクの解決・遷移・補完。

## ビルド構成

- **esbuild.js** — エントリ `src/extension.ts` → `dist/extension.js`、platform node、format cjs。プロダクションビルドは minify、開発ビルドは sourcemap を含む。`wasmCopyPlugin` が `parser_wasm_bg.wasm` を `dist/` へコピーする。`src/pkg/` は gitignore 対象の生成物。
- **tsconfig.json** — target ES2022、module Node16、strict モード有効。
- **eslint.config.mjs** — camelCase/PascalCase 命名、波括弧必須、厳密等価、throw リテラル禁止、セミコロン必須を強制。

## ビルド・配布フロー

- ビルド/チェックは `mise run` タスク（`build-wasm` / `build-cli` / `test-rust` / `compile` / `package` / `check` / `release`）で実行する。Rust ツールチェインは mise が解決する。
- `mise run release` でパッチバージョンを上げ、main と tags を push する。`v*` タグで GitHub Actions（`.github/workflows/release.yml`）が VSIX をビルドし GitHub Release を作成する。
