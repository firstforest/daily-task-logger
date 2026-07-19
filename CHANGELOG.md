# Change Log

All notable changes to the "taski" extension will be documented in this file.

Check [Keep a Changelog](http://keepachangelog.com/) for recommendations on how to structure this file.

## [0.0.34] - 2026-07-19

### Added

- プロジェクト状態に someday（棚上げ）を追加しました

## [0.0.33] - 2026-06-07

### Added

- タスク状態に「見送り」([-]) を追加しました。着手予定だったが着手しなかったタスクを表す状態で、表示やアラートは完了と同じ扱いですが、今日ビューでは表示しつつ進捗カウントの母数には含めません。`[ ]`→`[x]`→`[-]`→`[ ]` の3状態サイクルでトグルできます。
- スケジュール画面で `r` キーを押すと再描画して最新の状態に更新できるようにしました。

### Fixed

- `taskAlertLeadMinutes` の既定値の説明を実際の値（1分）に合わせて修正しました。

## [0.0.32] - 2026-05-03

### Added
- Markdown で `[[` を入力した際に、既存ノートを補完候補として表示するようにしました。

## [0.0.31] - 2026-04-25

### Added

- コードブロック内のタスクを解析対象から除外するようにしました
- `mise run release` で CHANGELOG を自動生成するようにしました

## [0.0.28] - 2026-04-15

### Added

- `[[foo]]` 形式の Wiki リンクナビゲーション（Cmd+Click で既存ファイルを開く、無ければ `$HOME/taski/note/` 配下に自動作成。`YYYY-MM-DD` はジャーナルとして扱う）
- CLI に `taski resolve` サブコマンドを追加（Wiki リンクのパス解決）
- front matter の `project` フラグに `active` / `done` の Status enum を導入

### Fixed

- Rust のバイトオフセットを UTF-16 に変換して VS Code の Range を構築
- Markdown 言語での拡張の自動活性化
- command URI のクエリを JSON 配列形式で渡すように修正

## [0.0.27] - 2026-04-12

### Added

- ファイル先頭の YAML front matter `project: active` を検出し、ファイル名を全タスクに自動タグ付け
- CLI `list --tag` が front matter 自動タグも対象

## [0.0.26] - 2026-04-12

### Changed

- コマンドの title に `Taski` プレフィックスを付与
- `mise download` / `install` タスクの改善

## [0.0.25] - 2026-04-11

### Added

- CLI に `schedule` サブコマンドを追加
- CLI に `agents-md` サブコマンドを追加
- `list` コマンドの出力に Markdown 見出し階層のコンテキストを追加
- `list` コマンドにタグフィルタオプションを追加

### Fixed

- tsconfig に mocha 型定義を追加してテストエラーを修正

## [0.0.24] - 2026-04-09

### Added

- タスク開始時刻のアラート通知機能（`taski.taskAlert` / `taski.taskAlertLeadMinutes`）

### Changed

- タグ別ビューで完了済みタスクを非表示に

## [0.0.23] - 2026-04-07

### Changed

- スケジュール表示のデフォルト時間範囲を 9:00–18:00 に変更し、範囲外のエントリがあれば自動拡張

## [0.0.22] - 2026-03-26

### Added

- ログ内の時間範囲（例: `13:00-14:00`）のパース対応

## [0.0.21] - 2026-03-26

### Added

- スケジュール表示で時間範囲の帯表示に対応

### Changed

- VS Code engine のバージョンを `^1.110.0` に更新

## [0.0.20] - 2026-03-21

### Added

- スケジュールグリッド機能（Webview, 15 分スロットの計画 / 実績ビュー）
- ジャーナルの時刻メモをスケジュールグリッドの実績列に表示
- ビルド・チェック用の `mise` タスクを追加

## [0.0.19] - 2026-03-21

### Fixed

- リリースタスクのエラーを修正

## [0.0.18] - 2026-03-20

### Added

- CLI（`taski` バイナリ）を新規追加。サブコマンド `memo` / `list` / `journal` / `toggle` を提供
- `list` に JSON / YAML 出力フォーマットオプション
- パーサーロジックを `parser-core` クレートに抽出し、CLI と WASM で共有
- `buildTreeData` WASM 関数を追加し、グルーピング・フィルタリング・ソートを Rust 側に移譲

## [0.0.17] - 2026-02-26

### Added

- タグ別 TreeView（`TagTreeProvider`）とタグ抽出ユーティリティ

## [0.0.16] - 2026-02-15

### Added

- Markdown でスラッシュコマンド（`/today`, `/tomorrow`, `/now`）による日付・時刻挿入

## [0.0.15] - 2026-02-15

### Added

- `$HOME/taski` ディレクトリの Git 自動同期機能（`GitSyncManager`）
- ジャーナルファイル作成時に日付を自動挿入

### Changed

- Git sync のデフォルト同期間隔を 30 秒から 60 秒に変更

## [0.0.14] - 2026-02-07

### Added

- 今日のタスクに進捗表示（`x/y`）を追加

## [0.0.13] - 2026-02-07

### Added

- TreeView のアイコンに色を追加（今日=緑、過去=オレンジ、完了=緑、未完了=黄色など）
- ワークスペースのスキャンを無効化する設定（`taski.includeWorkspace`）

## [0.0.12] - 2026-02-07

### Fixed

- ログエントリ挿入時のインデントを 4 スペースから 2 スペースに変更

## [0.0.11] - 2026-02-07

### Changed

- タスク一覧の表示を Webview から TreeView に変更

### Added

- 今日の日付のジャーナルファイルを開くコマンド（`taski.openTodayJournal`）

## [0.0.10] - 2026-02-05

### Fixed

- GitHub Actions に Rust と wasm-pack のセットアップを追加（リリースビルド修正）

## [0.0.9] - 2026-02-05

### Changed

- 拡張機能名を daily-task-logger から Taski に変更

### Added

- デフォルトで `$HOME/taski` ディレクトリをスキャン対象に含める

## [0.0.8] - 2026-02-04

### Changed

- パーサー関数を Rust WASM で再実装
- タスク一覧で未完了タスクを上に表示するように変更

## [0.0.7] - 2026-02-03

### Added

- タスクの完了状態をトグルするコマンド（`taski.toggleTask` / `Cmd+Shift+X`）

## [0.0.6] - 2026-02-03

### Added

- 明日のログエントリ追加コマンド（`taski.addTomorrowLog` / `Cmd+Shift+Y`）

## [0.0.5] - 2026-02-02

### Added

- Markdown ファイル保存時にタスクビューを自動更新
- 今日以外の日付で完了タスクを非表示にするフィルタリング

## [0.0.4] - 2026-02-02

### Added

- 今日のログエントリ追加コマンド（`taski.addTodayLog` / `Cmd+Shift+T`）
- 対象外ディレクトリ指定機能（`taski.excludeDirectories`）

## [0.0.3] - 2026-02-01

### Added

- 追加ディレクトリのスキャン設定（`taski.additionalDirectories`）

## [0.0.2] - 2026-02-01

### Added

- 全日付のタスクと日付なしタスクを表示する機能

### Fixed

- タスクリンクをクリック可能にするため WebviewPanel に移行

## [0.0.1] - 2026-02-01

- Initial release
