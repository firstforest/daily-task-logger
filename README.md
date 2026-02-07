# taski

ワークスペース内の Markdown ファイルからタスクとログを集約し、日付ごとに整理して表示する VS Code 拡張機能です。

## 機能

- ワークスペース内のすべての `.md` ファイルを走査し、タスク（`- [ ]` / `- [x]`）とその配下の日付付きログ（`- YYYY-MM-DD: テキスト`）を抽出します
- 今日のタスクを最優先で表示し、その下に他の日付のタスクを新しい順に表示します
- ログ行がないタスクも「日付なし」セクションとして表示されます
- 各タスクにはソースファイルへのクリック可能なリンクが付与され、該当行にジャンプできます
- 結果はアクティビティバーの専用アイコンからアクセスできる TreeView として表示されます
- Markdown ファイルの保存時にビューが自動更新されます

## コマンド

| コマンド                 | タイトル                 | キーバインド（Mac） | キーバインド（Win/Linux） |
| ------------------------ | ------------------------ | ------------------- | ------------------------- |
| `taski.showToday`        | Show Today's Tasks       | —                   | —                         |
| `taski.refreshTasks`     | Refresh Tasks            | —                   | —                         |
| `taski.addTodayLog`      | Add Today's Log Entry    | `Cmd+Shift+T`       | `Ctrl+Shift+T`            |
| `taski.addTomorrowLog`   | Add Tomorrow's Log Entry | `Cmd+Shift+Y`       | `Ctrl+Shift+Y`            |
| `taski.toggleTask`       | Toggle Task Completion   | `Cmd+Shift+X`       | `Ctrl+Shift+X`            |
| `taski.openTodayJournal` | Open Today's Journal     | —                   | —                         |

キーバインド付きのコマンドは、Markdown ファイルの編集中（`editorTextFocus && editorLangId == markdown`）のみ有効です。

### Show Today's Tasks

コマンドパレット（`Ctrl+Shift+P` / `Cmd+Shift+P`）から実行すると、TreeView にタスク一覧を表示します。

### Refresh Tasks

TreeView のタイトルバーにあるリフレッシュボタン、またはコマンドパレットから実行すると、タスク一覧を再スキャンして更新します。

### Add Today's Log Entry / Add Tomorrow's Log Entry

カーソルがタスク行またはその配下のログ行にある状態で実行すると、今日（または明日）の日付でログエントリの行を挿入します。

### Toggle Task Completion

カーソルがタスク行にある状態で実行すると、`[ ]` と `[x]` をトグルします。

### Open Today's Journal

今日の日付のジャーナルファイルを開きます。

## 設定

| 設定                          | 型         | デフォルト | 説明                                                                            |
| ----------------------------- | ---------- | ---------- | ------------------------------------------------------------------------------- |
| `taski.includeWorkspace`      | `boolean`  | `false`    | 現在のワークスペースをスキャン対象に含めるかどうか                              |
| `taski.excludeDirectories`    | `string[]` | `[]`       | スキャン対象から除外するディレクトリの glob パターン一覧（例: `**/archive/**`） |
| `taski.additionalDirectories` | `string[]` | `[]`       | 追加でスキャンするディレクトリのパス一覧（絶対パス）                            |

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
```

- ログ行は親タスク行よりも深いインデントが必要です
- すべての日付のログが表示されます（今日 → 新しい順 → 日付なし）

## 要件

- VS Code 1.108.1 以上

## 開発

```bash
npm install
npm run compile    # 型チェック + lint + ビルド（dev）
npm run watch      # esbuild + tsc の並列ウォッチ
npm run package    # 型チェック + lint + 本番ビルド（minify）
npm run test       # テスト実行（VS Code インスタンスが起動します）
```
