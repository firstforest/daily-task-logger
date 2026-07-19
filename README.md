# taski

ワークスペース内の Markdown ファイルからタスクとログを集約し、日付ごとに整理して表示する VS Code 拡張機能です。

## 機能

- ワークスペース内のすべての `.md` ファイルを走査し、タスク（`- [ ]` 未完了 / `- [x]` 完了 / `- [-]` 見送り）とその配下の日付付きログ（`- YYYY-MM-DD: テキスト`）を抽出します
- 今日のタスクを最優先で表示し、その下に他の日付のタスクを新しい順に表示します
- ログ行がないタスクも「日付なし」セクションとして表示されます
- 各タスクにはソースファイルへのクリック可能なリンクが付与され、該当行にジャンプできます
- 結果はアクティビティバーの専用アイコンからアクセスできる TreeView として表示されます
- Markdown ファイルの保存時にビューが自動更新されます
- Markdown 内でスラッシュコマンド（`/today`, `/tomorrow`, `/now`）を入力すると、日付・時刻を挿入できます
- `$HOME/taski` ディレクトリの Git 自動同期（add → commit → pull --rebase → push）

## コマンド

| コマンド                 | タイトル                 | キーバインド（Mac） | キーバインド（Win/Linux） |
| ------------------------ | ------------------------ | ------------------- | ------------------------- |
| `taski.showToday`        | Show Today's Tasks       | —                   | —                         |
| `taski.refreshTasks`     | Refresh Tasks            | —                   | —                         |
| `taski.addTodayLog`      | Add Today's Log Entry    | `Cmd+Shift+T`       | `Ctrl+Shift+T`            |
| `taski.addTomorrowLog`   | Add Tomorrow's Log Entry | `Cmd+Shift+Y`       | `Ctrl+Shift+Y`            |
| `taski.toggleTask`       | Toggle Task Status       | `Cmd+Shift+X`       | `Ctrl+Shift+X`            |
| `taski.openTodayJournal` | Open Today's Journal     | —                   | —                         |
| `taski.syncNow`          | Sync Now (Git)           | —                   | —                         |

キーバインド付きのコマンドは、Markdown ファイルの編集中（`editorTextFocus && editorLangId == markdown`）のみ有効です。

### Show Today's Tasks

コマンドパレット（`Ctrl+Shift+P` / `Cmd+Shift+P`）から実行すると、TreeView にタスク一覧を表示します。

### Refresh Tasks

TreeView のタイトルバーにあるリフレッシュボタン、またはコマンドパレットから実行すると、タスク一覧を再スキャンして更新します。

### Add Today's Log Entry / Add Tomorrow's Log Entry

カーソルがタスク行またはその配下のログ行にある状態で実行すると、今日（または明日）の日付でログエントリの行を挿入します。

### Toggle Task Status

カーソルがタスク行にある状態で実行すると、`[ ]`（未完了）→ `[x]`（完了）→ `[-]`（見送り）→ `[ ]` の順に状態を切り替えます。

`[-]`（見送り）は「着手予定だったが着手できなかったタスク」を表します。やらないと決めたか、翌日や別のリストへ積み替えた後にこの状態にする運用を想定しています。表示は完了と同じく扱われ（他の日付・タグ別ビューでは非表示、スケジュールでは打ち消し線）、今日のビューには表示されますが進捗カウントの母数には含まれません。

### Open Today's Journal

今日の日付のジャーナルファイル（`$HOME/taski/journal/<year>/<month>/<year>-<month>-<day>.md`）を開きます。ディレクトリが存在しない場合は自動作成されます。

### Sync Now (Git)

`$HOME/taski` ディレクトリの Git 同期を手動実行します。TreeView のタイトルバーに表示されます（Git 自動同期が有効な場合のみ）。

### スラッシュコマンド

Markdown ファイルの編集中に以下のスラッシュコマンドが補完候補として表示されます:

| コマンド     | 挿入される内容                   |
| ------------ | -------------------------------- |
| `/today`     | 今日の日付（`YYYY-MM-DD`）       |
| `/tomorrow`  | 明日の日付（`YYYY-MM-DD`）       |
| `/now`       | 現在の時刻（`HH:mm`）           |

## 設定

| 設定                          | 型         | デフォルト | 説明                                                                            |
| ----------------------------- | ---------- | ---------- | ------------------------------------------------------------------------------- |
| `taski.includeWorkspace`      | `boolean`  | `false`    | 現在のワークスペースをスキャン対象に含めるかどうか                              |
| `taski.excludeDirectories`    | `string[]` | `[]`       | スキャン対象から除外するディレクトリの glob パターン一覧（例: `**/archive/**`） |
| `taski.additionalDirectories` | `string[]` | `[]`       | 追加でスキャンするディレクトリのパス一覧（絶対パス）                            |
| `taski.gitAutoSync`           | `boolean`  | `true`     | `$HOME/taski` ディレクトリの Git 自動同期を有効にする                           |
| `taski.gitSyncInterval`       | `number`   | `60`       | Git 自動同期の間隔（秒）。最小 30 秒                                           |

### デフォルトスキャンディレクトリ

ワークスペースに加えて、`$HOME/taski` ディレクトリ（Windows では `%USERPROFILE%\taski`）が存在する場合、
自動的にスキャン対象に含まれます。これにより、ワークスペース外の個人的なタスクファイルも一元管理できます。

## タスクの書き方

Markdown ファイルに以下の形式でタスクとログを記述します:

```markdown
- [x] タスク名
    - 2026-02-01: この日のログエントリ
- [ ] 別のタスク
    - 2026-02-01: ログエントリ
    - 2026-01-31: 別の日のログ
- [ ] ログなしのタスク
- [-] 見送ったタスク
    - 2026-02-01: 今日は着手できなかったので翌日へ繰り越し
```

- ログ行は親タスク行よりも深いインデントが必要です
- すべての日付のログが表示されます（今日 → 新しい順 → 日付なし）

### ファイル単位の自動タグ

Markdown ファイルの先頭に YAML front matter で `project: active` を記述すると、そのファイル名（`.md` 拡張子を除き、空白は `_` に置換）がタグとして全タスクに自動付与されます（本文中の `#tag` 記法と合算されます）。

```markdown
---
project: active
---

- [ ] レビュー #urgent
    - 2026-04-12: 確認中
```

ファイル名が `projectA.md` の場合、上記タスクは `#projectA` `#urgent` の 2 タグとして扱われ、タグ別ビューや CLI の `--tag` フィルタでヒットします。`project` が無い、または `someday`（着手を棚上げしたプロジェクト）・`done`（完了済みプロジェクト）の場合は本文中の `#tag` だけが対象になります。

## 要件

- VS Code 1.108.1 以上

## 開発

### 前提条件

- **Rust ツールチェイン** — `rustc` と `cargo`（[rustup](https://rustup.rs/) でインストール）
- **wasm-pack** — `cargo install wasm-pack`
- **wasm32-unknown-unknown ターゲット** — `rustup target add wasm32-unknown-unknown`

### ビルド

```bash
npm install
npm run compile    # WASM ビルド + 型チェック + lint + esbuild（dev）
npm run watch      # esbuild + tsc の並列ウォッチ（WASM は再ビルドされません）
npm run package    # WASM ビルド + 型チェック + lint + 本番ビルド（minify）
npm run test       # テスト実行（VS Code インスタンスが起動します）
```
