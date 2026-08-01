# taski CLI

`$HOME/taski/` 配下のMarkdownファイルからタスクを管理するCLIツール。

## データディレクトリ

すべてのコマンドは `$HOME/taski/` を基準ディレクトリとして動作する。ジャーナルファイルは `$HOME/taski/journal/<year>/<month>/<YYYY-MM-DD>.md` に保存される。

## タスクのMarkdownフォーマット

```markdown
- [ ] 未完了タスク #tag1
    - 2026-04-11: ログエントリ
- [x] 完了済みタスク #tag2
    - 2026-04-10: 作業ログ
```

- タスク行: `- [ ]`(未完了) または `- [x]`(完了)
- ログ行: タスクよりインデントが深い `- YYYY-MM-DD: テキスト`
- タグ: タスクテキスト内の `#タグ名` パターン（スペースや`#`を含まない文字列）

## コマンド

### `taski memo <text>`

今日のジャーナルファイルにタイムスタンプ付きメモを追記する。

```bash
# 基本的な使い方
taski memo 会議のメモ
# => "- 14:30: 会議のメモ" が追記される

# タイムスタンプなし
taski memo --no-timestamp 買い物リスト
# => "- 買い物リスト" が追記される

# パイプで入力
echo "パイプからの入力" | taski memo
```

**オプション:**
- `--no-timestamp` — 時刻プレフィックスを付けない
- テキスト引数を省略した場合、stdinから読み取る（パイプ入力時のみ）

### `taski list`

`$HOME/taski/` 内のすべてのMarkdownファイルからタスクを収集し、日付別にグループ化して表示する。

```bash
# デフォルト表示（色付きテキスト）
taski list

# JSON形式で出力
taski list --format json

# YAML形式で出力
taski list --format yaml

# 特定のタグでフィルタ
taski list --tag work

# タグフィルタとJSON出力の組み合わせ
taski list --tag work --format json
```

**オプション:**
- `-f, --format <FORMAT>` — 出力フォーマット（`json` または `yaml`）
- `-t, --tag <TAG>` — 指定タグを含むタスクのみ表示（`#` は不要、例: `--tag work`）

ファイル冒頭の YAML front matter に `project: active` を指定した場合、そのファイル名（`.md` 拡張子を除き、空白は `_` に置換）がタグとして全タスクに自動付与され、`--tag` フィルタの対象になる。`project: done`（完了済みプロジェクト）や `project: someday`（棚上げ）、未指定の場合は自動タグ付けされない。

`--format json` / `--format yaml` は該当0件でも空の配列（`[]`）を返す（終了コードは 0）。`someday` / `done` の PJ 名でタグを引くと 0 件になるのは正常系なので、そのままパースしてよい。ただし `~/taski` 自体が無い場合は終了コード 1 で stderr にメッセージを出す（stdout は空）ので、パースの前に終了コードは見ること。

**表示ルール:**
- 今日の日付のタスクは完了・未完了の両方を表示
- それ以外の日付は未完了タスクのみ表示
- ログのないタスクは「日付なし」グループに表示

**JSON出力の構造:**

```json
[
  {
    "dateKey": "2026-04-11",
    "label": "今日 (2026-04-11) (2/5)",
    "isToday": true,
    "completedCount": 2,
    "totalCount": 5,
    "fileGroups": [
      {
        "fileName": "journal/2026/04/2026-04-11.md",
        "fileUri": "/Users/user/taski/journal/2026/04/2026-04-11.md",
        "tasks": [
          {
            "status": "incomplete",
            "text": "タスク名 #tag",
            "fileUri": "/Users/user/taski/journal/2026/04/2026-04-11.md",
            "line": 3,
            "log": "ログ内容",
            "date": "2026-04-11",
            "context": ["見出し"]
          }
        ]
      }
    ]
  }
]
```

### `taski schedule`

今日（または指定日）のスケジュールを時間割形式で表示する。タスクのログ行に含まれる時刻情報と、ジャーナルファイルの時刻メモを集約して時系列順に表示する。

```bash
# 今日のスケジュールを表示
taski schedule

# 特定の日付のスケジュールを表示
taski schedule --date 2026-04-10

# JSON形式で出力
taski schedule --format json

# YAML形式で出力
taski schedule --format yaml
```

**オプション:**
- `-f, --format <FORMAT>` — 出力フォーマット（`json` または `yaml`）
- `-d, --date <DATE>` — 表示する日付（`YYYY-MM-DD` 形式、省略時は今日）

`list` と同じく、`--format json` / `--format yaml` は該当0件でも空の配列（`[]`）を返す（終了コードは 0）。予定の無い日は正常系として普通に起きるので、そのままパースしてよい。

**表示内容:**
- 時刻付きタスク（`HH:MM` または `HH:MM-HH:MM`）は時刻順に表示
- 時刻なしタスクは `--:--` として末尾に表示
- ジャーナルメモ（タスクに紐づかない `- HH:MM: テキスト` 行）も表示
- 完了状態（`[x]`/`[ ]`）を色分け表示

**JSON出力の構造:**

```json
[
  {
    "taskText": "API設計レビュー",
    "taskLine": 3,
    "status": "incomplete",
    "logText": "エンドポイント設計の確認",
    "logLine": 4,
    "time": "10:00",
    "endTime": "11:00",
    "fileUri": "/Users/user/taski/journal/2026/04/2026-04-11.md"
  }
]
```

- `taskText` — タスク名（ジャーナルメモの場合は空文字列）
- `status` — `incomplete` / `completed` / `cancelled`
- `time` — 開始時刻（時刻なしの場合は空文字列）
- `endTime` — 終了時刻（範囲指定なしの場合は空文字列）
- `logText` — ログ内容またはメモのテキスト

### `taski journal`

今日のジャーナルファイルを `$EDITOR` で開く。ファイルが存在しない場合は自動作成する。

```bash
# エディタで開く
taski journal

# パスだけ表示（エディタを開かない）
taski journal --print
# => /Users/user/taski/journal/2026/04/2026-04-11.md
```

**オプション:**
- `--print` — ファイルパスを標準出力に表示するだけ（エディタを起動しない）

`$EDITOR` が未設定の場合はパス表示にフォールバックする。

### `taski toggle <file> <line>`

指定ファイルの指定行にあるタスクの完了状態を切り替える（`[ ]` ↔ `[x]`）。

```bash
# 3行目のタスクをトグル
taski toggle ~/taski/tasks.md 3
```

**引数:**
- `<file>` — 対象Markdownファイルのパス
- `<line>` — 行番号（1始まり）

`list --format json` の出力に含まれる `fileUri` と `line` をそのまま使える。

### `taski resolve <name>`

`[[name]]` に対応するファイルパスを解決して出力する。`$HOME/taski` → workspace → 追加ディレクトリ → 開いているドキュメントの優先順位で既存の `<name>.md` を探す。見つからない場合は新規作成して出力する。

```bash
# wiki リンク先を解決（なければ作成）
taski resolve foo
# => /Users/user/taski/note/foo.md

# 日付形式はジャーナルとして扱う
taski resolve 2026-04-14
# => /Users/user/taski/journal/2026/04/2026-04-14.md

# 作成しない（見つからなければ非ゼロ終了）
taski resolve foo --no-create

# JSON形式で出力
taski resolve foo --format json
```

**オプション:**
- `--no-create` — ファイルが見つからなくても作成せず、非ゼロで終了する
- `-f, --format <FORMAT>` — 出力フォーマット（`json`）

**JSON出力の構造:**

```json
{
  "name": "foo",
  "path": "/Users/user/taski/note/foo.md",
  "created": true
}
```

- `name` — 解決したリンク名
- `path` — ファイルの絶対パス
- `created` — 新規作成した場合 `true`、既存ファイルの場合 `false`

### `taski pj`

PJ（プロジェクト）横断の状態を集約して表示する。`list` が日付軸なのに対し、こちらは PJ 軸。

対象は `$HOME/taski/note/*.md` のうち、front matter に `project:` を持つノート。PJ ノートは次の 3 セクションからなる日報形式で書かれている。

```markdown
---
project: active          # active / someday / done
repo: ~/workspace/foo    # 任意。作業実体が別リポジトリにある場合のパス
---
# PJ名

## 次の予定
- [ ] 続きを実装する（30分・重・@PC）   # 唯一の Next Action。1行だけ

## ログ
- 2026-07-30: ここまでやった

## オープンタスク
- あとで考える                          # バックログ。チェックボックスは付けない
```

**このコマンドは判断をしない。** 機械的に決まる事実だけを出す。「このタスクは粒度が粗い」「これは着手すべき」といった判断は呼び出し側（skill やエージェント）が行う。

```bash
# 既定（table 形式、active な PJ のみ、repo を fetch してから読む）
taski pj

# JSON で取得（エージェントから使う場合はこちら）
taski pj --format json

# fetch を省略して即答（repo の日付はローカルの clone 基準になる）
taski pj --no-fetch --format json

# 棚上げ・完了も含めて見る
taski pj --status active,someday
taski pj --all

# 基準日を指定（基準日より後の日付を持つ情報を集計から除外する）
taski pj --today 2026-07-25
```

**オプション:**
- `-f, --format <FORMAT>` — 出力フォーマット（`table` または `json`、既定は `table`）
- `-s, --status <STATUS>` — カンマ区切りで status を絞る（`active` / `someday` / `done`、既定は `active`）
- `--all` — status で絞らず全件表示（`done` を含む。`--status` とは排他）
- `--today <DATE>` — 基準日（`YYYY-MM-DD`、省略時は今日）。経過日数の計算と、後述の日付フィルタに使う
- `--no-fetch` — `repo:` のリポジトリを `git fetch` しない

**fetch について:**

既定で `repo:` のリポジトリを `git fetch` してから読む。fetch しない限り「ローカルの clone が古い」ことは検出できない（remote-tracking ref 自体が古く、比較対象にならない）ため。fetch のみで `pull` はしないので、作業ツリーと HEAD は動かない。ネットワークを使うぶん遅くなるので、**即答性が要る用途（「次に何をやる？」に答えるなど）では `--no-fetch` を使う**。その場合 `repo_last` はローカルの clone 基準になる。

fetch に失敗しても集計は続行し、失敗したリポジトリを `fetch_failed` に挙げる。ここに挙がった PJ の `repo_last` は信用できない。リモートを持たないローカル専用リポジトリは fetch をスキップする（失敗ではない）。

**`--today` について:**

基準日より後のログ・コミット・journal 言及・ノート更新は「その時点ではまだ無い」ものとして集計から除外する。`log_last` などは基準日以前の最新を採り、無ければ `null` になる。経過日数が負になることはない。先の日付を書いたログや翌日ぶんの journal も同じ扱いなので、基準日を省略した通常運用でも負にならない。

**巻き戻るのは日付由来のフィールドだけ。** `log_last` / `repo_last` / `journal_last` / `updated` とそれぞれの日数、および `logs` が対象。`next_action` / `health` / `backlog` / `status` / `completed` は **ノートの現在の内容がそのまま出る**（過去のノート内容は git から復元しない）。過去日を渡しても「その日時点のスナップショット」にはならないので、`--status` の絞り込みも現在の `project:` の値で効く。

**JSON出力の構造:**

```json
{
  "generated": "2026-08-01",
  "fetched": true,
  "fetch_failed": [],
  "projects": [
    {
      "name": "漫画制作エディタ",
      "path": "note/漫画制作エディタ.md",
      "status": "active",
      "repo": "~/workspace/manga-editor",
      "completed": null,
      "next_action": "エクスポート処理を書く（45分・重・@PC）",
      "next_action_body": "エクスポート処理を書く",
      "next_action_meta": "45分・重・@PC",
      "next_action_ai": false,
      "health": "ok",
      "updated": "2026-07-30",
      "stale_days": 2,
      "log_last": "2026-07-28",
      "log_days": 4,
      "repo_last": "2026-07-31",
      "repo_days": 1,
      "unreported": true,
      "unreported_count": 3,
      "journal_last": "2026-07-29",
      "journal_days": 3,
      "backlog_count": 2,
      "backlog": ["ページ管理を整理する", "書き出し設定を保存する"],
      "logs": [
        { "date": "2026-07-28", "text": "コマ割りの実装" }
      ]
    }
  ]
}
```

トップレベル:
- `generated` — 基準日（`--today` の値、省略時は今日）
- `fetched` — `repo:` を fetch したか。`false` のとき `repo_last` は古い可能性がある
- `fetch_failed` — fetch に失敗したリポジトリのパス
- `projects` — PJ の配列（該当0件でも空配列を返す）

`projects[]` の各フィールド:
- `name` — PJ 名（ノートのファイル名から拡張子を除いたもの）
- `path` — `$HOME/taski` からの相対パス
- `status` — `active` / `someday` / `done`
- `repo` — front matter の `repo:`（未設定なら `null`）
- `completed` — 完了日（`done` のときのみ。未設定なら `null`）
- `next_action` — `## 次の予定` の最初の `- [ ]` 行（メタデータ込みの原文）
- `next_action_body` / `next_action_meta` — 本文と判断メタデータ（`45分・重・@PC`）に分離したもの
- `next_action_ai` — 判断メタデータのコンテキストが `@AI` か
- `health` — `ok` / `unclarified`（判断メタデータが無い）/ `no-next`（`- [ ]` が無い）
- `updated` / `stale_days` — PJ ノート自体の最終更新日（git 基準）と経過日数
- `log_last` / `log_days` — `## ログ` の最新日付と経過日数
- `repo_last` / `repo_days` — `repo:` のリポジトリの最終コミット日と経過日数
- `unreported` / `unreported_count` — `repo_last > log_last` のとき `true`。作業は進んでいるのにログに反映されていない状態と、その未反映コミット数
- `journal_last` / `journal_days` — journal で `[[PJ名]]` / `#PJ名` により最後に言及された日と経過日数
- `backlog_count` / `backlog` — `## オープンタスク` の項目
- `logs` — 直近のログ（最大3件、新しい順）。再開時のコンテキストとして使う

日付が取れない場合（ログが1件も無い、`repo:` のパスが存在しない等）は該当フィールドが `null` になる。停滞（`stale_days` / `log_days`）と言及（`journal_days`）は別々に持つ。合成すると「候補に載っただけで停滞0日」になり実態が見えなくなるため。

**table の並び順:**

手を入れる必要が高い順（未反映 → ログが古い/無い → `no-next` → `unclarified`）。未反映の PJ は行頭に `!` が付く。

## 終了コード

- `0` — 成功
- `1` — エラー（メッセージはstderrに出力）

## 典型的なワークフロー

```bash
# 今日のスケジュールを確認
taski schedule

# 今日のタスクを確認
taski list

# 特定プロジェクトのタスクだけ確認
taski list --tag myproject

# PJ 横断の状態を確認（即答が要るときは --no-fetch）
taski pj
taski pj --no-fetch --format json | jq -r '.projects[] | select(.unreported) | .name'

# メモを追記
taski memo MTGで決まったこと: デプロイは来週

# タスクを完了にする
taski toggle ~/taski/journal/2026/04/2026-04-11.md 5

# 他のツールと連携（JSON出力をjqで加工）
taski list --format json | jq -r '.[].fileGroups[].tasks[] | select(.status == "incomplete") | .text'

# スケジュールの空き時間を確認
taski schedule --format json | jq '[.[] | select(.time != "")] | sort_by(.time)'
```
