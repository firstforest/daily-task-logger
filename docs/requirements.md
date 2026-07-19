# taski 要求仕様

本ドキュメントは、現状の実装とドキュメント（`CLAUDE.md`、`docs/superpowers/` 配下の設計資料、`package.json`、Rust パーサー）から、taski に対する要求を整理したものである。実装の現状を反映した「あるべき仕様」の記述であり、個々の実装詳細ではなく満たすべき要件を示す。

構成・設計方針については [architecture.md](architecture.md) を参照。

## 1. 概要・目的

- taski は、ワークスペースやユーザーのホーム配下に散在する Markdown ファイルからタスクを集約し、日付ごと・タグごとに整理して表示する VS Code 拡張である。
- タスクは元の Markdown ファイルへのクリック可能なリンクとして提示され、ソースへ即座に遷移できること。
- 同じ機能をターミナルから利用できる Rust 製 CLI (`taski`) を併せて提供する。
- UI 文字列・コードコメントは日本語とする。

## 2. タスク Markdown フォーマット要求

### 2.1 タスクとログ

- タスク行は `- [ ]`（未完了）/ `- [x]`（完了）/ `- [-]`（見送り）で表す。
  - `- [-]`（見送り）は「着手予定だったが着手できなかったもの」を表す。やらないと決めたか、翌日・inbox・nextAction などに同じものを積み替えた後にこの状態にする運用を想定する。表示上は「完了と同じく対応不要（クローズ済み）」として扱うが、完了とは区別する（[3.1](#31-treeview) 参照）。
- タスク行の下に、より深くインデントされた `- YYYY-MM-DD: ログ本文` 行を記述でき、これをそのタスクの当該日付のログとする。
- ログ行はタスク行より深いインデントであることを必須とする。
- ログを 1 つも持たないタスクは「日付なし」として扱う。
- コードブロック（``` / ~~~ で囲まれた範囲）内のタスク・ログは解析対象から除外すること。

### 2.2 ファイル単位の自動タグ

- ファイル先頭の YAML front matter に `project: active` がある場合、そのファイル名（`.md` 拡張子を除去し、空白を `_` に置換した文字列）をタグとして全タスクに自動付与する。
- 本文中の `#tag` と自動タグは合算される。
- `project` が未指定、または `project: someday` / `project: done` の場合は自動タグを付与せず、本文の `#tag` のみを用いる（`someday` は着手を棚上げしたプロジェクト、`done` は完了済みプロジェクトを示すメタ情報）。

### 2.3 時刻付きログ／スケジュール記法

- ログ行に時刻を付与できる: `- YYYY-MM-DD HH:MM: 本文`、開始終了の指定 `- YYYY-MM-DD HH:MM-HH:MM: 本文` も可能。
- ジャーナルファイルでは日付見出し `# YYYY-MM-DD` 配下のトップレベル時刻メモ `- HH:MM: 本文` をその日のスケジュール項目として扱う。
- 時刻は 1〜2 桁時刻を 2 桁にパディングして正規化する。

## 3. VS Code 拡張の機能要求

### 3.1 TreeView

- 日付別ビュー（`taskiView` / 「タスク一覧」）: 日付 → ファイル → タスク → ログ の階層で表示する。
  - 並び順は「今日」を先頭とし、以降は新しい日付から古い順とする。
  - 「今日」は全タスク（完了・見送り含む）を進捗カウンタ付きで表示し、それ以外の日付は未完了タスクのみ表示する（完了・見送りは非表示）。
  - 見送り（`- [-]`）は「今日」ビューに表示するが、進捗カウンタ（`完了/総数`）の分母・分子どちらにも含めない中立状態として扱う。表示順は 未完了 → 完了 → 見送り とする。
- タグ別ビュー・スケジュール・アラートでは見送りを完了と同じく扱う（タグ別では非表示、スケジュールでは打ち消し線、アラート対象外）。
- タグ別ビュー（`taskiTagView` / 「タグ別」）: タスク本文の `#tag` とファイル単位の自動タグでグルーピングして表示する。
- 各ビューのタイトルから「更新」「同期（Git 自動同期有効時のみ）」「スケジュール表示」を操作できること。
- タスク／ログ項目のクリックで、元ファイルの該当行へ遷移できること。

### 3.2 コマンド

ユーザー向けコマンド:

- `taski.showToday` — 今日のタスクを表示
- `taski.refreshTasks` — タスクの再スキャン・再表示
- `taski.addTodayLog` — 今日の日付のログ行を追記（既定キー: Cmd/Ctrl+Shift+T）
- `taski.addTomorrowLog` — 明日の日付のログ行を追記（既定キー: Cmd/Ctrl+Shift+Y）
- `taski.toggleTask` — タスクの状態を切り替え（`[ ]` → `[x]` → `[-]` → `[ ]` の3状態サイクル、既定キー: Cmd/Ctrl+Shift+X）
- `taski.openTodayJournal` — 今日のジャーナルを開く
- `taski.syncNow` — Git を即時同期
- `taski.showSchedule` — スケジュールグリッドを表示
- `taski.openWikiLinkAtCursor` — カーソル位置の Wiki リンクを開く（既定キー: Cmd/Ctrl+Shift+O）

内部コマンド（コマンドパレット非表示）:

- `taski.openTaskLocation` — 指定ファイル・行を開く
- `taski.openWikiLink` — Wiki リンク先を開く

キーバインドは Markdown エディタにフォーカスがある場合のみ有効とする。

### 3.3 スラッシュ補完

- Markdown 編集中に `/today` `/tomorrow` `/now` を入力すると、それぞれ今日の日付・明日の日付・現在時刻を挿入する補完候補を提供する。
- すべての日付・時刻はローカルタイムゾーンで算出すること（UTC 不可）。

### 3.4 Wiki リンクナビゲーション

- Markdown 内の `[[foo]]` を Cmd/Ctrl+Click または専用コマンドで開けること。
- 解決優先順位: `$HOME/taski` > ワークスペース > 追加ディレクトリ > 開いているドキュメント。
- 一致する `foo.md` が存在しない場合は `$HOME/taski/note/foo.md` を `# foo\n` を初期内容として作成して開く。
- `[[YYYY-MM-DD]]` 形式はジャーナル `$HOME/taski/journal/<YYYY>/<MM>/<YYYY-MM-DD>.md` として扱う。
- `[[` 入力時に既存ノートを補完候補として提示すること。

### 3.5 スケジュールグリッド

- WebviewPanel で 15 分刻みのタイムグリッドを表示する。
- 表示時間帯は対象日のスケジュール項目に応じて動的に決定する。
- 計画（plan）と実績（actual）を対比できる構成とすること。

### 3.6 タスクアラート

- タスクの開始時刻が近づいた際に通知を表示する（`taski.taskAlert` で有効/無効）。
- 開始の何分前に通知するかを `taski.taskAlertLeadMinutes`（既定 1、最大 30）で設定できる。
- 定期的（30 秒間隔）に対象を判定し、同一タスクの重複通知を防止する。日付が変わったら通知済み状態をリセットする。

### 3.7 Git 自動同期

- `$HOME/taski` が Git リポジトリの場合、一定間隔で自動同期する（`add -A` → `commit` → `pull --rebase` → `push`）。
- ファイル保存時にもデバウンス付きで同期する。
- `taski.gitAutoSync` で有効/無効、`taski.gitSyncInterval`（秒、既定 60、最小 30）で間隔を設定できる。
- 変更がない場合はコミットをスキップする。同時実行を防止する。
- コンフリクト発生時は rebase を中断して作業ツリーを復元し、自動同期タイマーを停止してユーザーに通知する。ネットワークエラー時はログのみ記録し次回リトライする。

## 4. ファイルスキャン要求

- 既定で `$HOME/taski` が存在すれば常にスキャン対象とする（設定に依存しない）。
- 現在開いている Markdown ドキュメントは常にスキャン対象に含める。
- ワークスペースのスキャンは `taski.includeWorkspace`（既定 `false`）が有効な場合のみ行う。
- `taski.additionalDirectories`（絶対パス一覧）で追加スキャン対象を指定できる。
- `taski.excludeDirectories`（glob パターン一覧）でスキャン除外を指定できる。`node_modules` は常に除外する。

## 5. ジャーナル要求

- ジャーナルのパスは `$HOME/taski/journal/<year>/<month>/<YYYY-MM-DD>.md` とする。
- 必要なディレクトリは自動生成する。
- 拡張の `openTodayJournal` と CLI の `journal` サブコマンドで同一パス規約を用いること。

## 6. CLI (`taski`) 要求

- `parser-core` を直接利用し（WASM を介さない）、`clap` で構築する。
- サブコマンド:
  - `memo <text>` — 今日のジャーナルへ `- HH:MM: text` を追記。`--no-timestamp` で時刻接頭辞を省略。テキスト未指定かつパイプ入力時は stdin から読む。
  - `list` — 日付別にタスクを表示。`--format json|yaml` で構造化出力、`--tag <tag>` でタグ絞り込み。
  - `journal` — 今日のジャーナルを `$EDITOR` で開く。`--print` でパスのみ出力。
  - `toggle <file> <line>` — 指定ファイル・行（1 始まり）のタスク状態を切り替え（`[ ]` → `[x]` → `[-]` → `[ ]` の3状態サイクル）。
  - `schedule` — 今日のスケジュールを表示。`--format json|yaml`、`--date YYYY-MM-DD` で日付指定。
  - `agents-md` — 同梱の AGENTS.md 内容を出力。`--output <path>` でファイル書き出し。
  - `resolve <name>` — `[[name]]` の対応ファイルパスを出力。無ければ作成（日付なら journal 配下、それ以外は `$HOME/taski/note/<name>.md`）。`--no-create` で作成抑止、`--format json` で構造化出力。

## 7. 設定一覧（要求としての既定値）

| 設定キー | 型 | 既定 | 内容 |
| --- | --- | --- | --- |
| `taski.includeWorkspace` | boolean | `false` | 現在のワークスペースをスキャン対象に含めるか |
| `taski.excludeDirectories` | array | `[]` | スキャン除外ディレクトリの glob パターン |
| `taski.additionalDirectories` | array | `[]` | 追加スキャン対象ディレクトリ（絶対パス） |
| `taski.taskAlert` | boolean | `true` | タスク開始時刻のアラート通知 |
| `taski.taskAlertLeadMinutes` | number | `1`（最大 30） | 開始何分前に通知するか |
| `taski.gitAutoSync` | boolean | `true` | `$HOME/taski` の Git 自動同期 |
| `taski.gitSyncInterval` | number | `60`（最小 30） | Git 自動同期間隔（秒） |

> ビルド・配布に関する要求は [architecture.md](architecture.md) の「ビルド構成」「ビルド・配布フロー」を参照。
