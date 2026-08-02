# taski 設計 — ドメインモデル

本ドキュメントは taski のドメインを Rust の型としてどう表しているかを形式的に記述する。

- **[requirements.md](requirements.md)** — *何を* 満たすか（要求）
- **[architecture.md](architecture.md)** — *どこに* 置くか（クレート構成・パイプライン・ビルド）
- **本ドキュメント** — *どう表すか*（型・不変条件・写像・事前条件）

要求の根拠（なぜその仕様なのか）は requirements.md 側にある。ここでは要求を型と述語に落とした結果だけを扱い、根拠は重複させない。

## 1. 設計原則

**P1. 解析は全域かつ純粋な写像である。**
`parser-core` は入力 `Vec<String>`（行の列）から出力データ構造への写像だけを担う。ファイルシステム・git・時刻・環境変数に触れない。任意の入力に対して停止し、パニックせず、値を返す（例外は §7 の事前条件のみ）。これにより VS Code 拡張（WASM 経由）と CLI（直接リンク）で同じ入力から同じ出力が出ることが型レベルで保証される。

**P2. 副作用は `cli` に閉じる。**
git の呼び出し・ディレクトリ走査・並列 fetch・現在日付の取得は `cli/src/pj.rs` に置く。`parser-core` の関数は「基準日」「ファイルの日付」を **引数で受け取る**（`journal_work(lines, file_date)`, `build_tree_data_internal(files, today_str)`）。テストが時刻に依存しないのはこの分割の帰結である。

**P3. 事実と判断を型で分ける。**
`PjProject` が持つのは機械的に決まる事実（日付・件数・有無）だけで、「着手すべきか」「粒度が粗いか」は含めない。唯一の例外が `PjHealth` で、これは §6 I-11 のとおり他フィールドから決定的に導出される要約であり、独立した判断ではない。

**P4. 同じ概念は 1 つの関数から出す。**
参照の抽出（`collect_refs`）、判断メタデータの分離（`split_decision_meta`）は言及側・実働側・`list` 側で同一の関数を共有する。片方だけが表記の揺れを拾うと、§6 I-17 のような不変条件が壊れる。

**P5. 境界（WASM / JSON）を通る型は素直な表現に寄せる。**
`serde` の既定表現で往復できることを優先し、newtype やタグ付き列挙を境界型には持ち込まない。その代償として本来は直和である概念が「フラットな構造体＋空文字列の判別子」に潰れている箇所がある（§5.3・§9）。潰した箇所は必ず本ドキュメントで代数的な形を併記する。

## 2. ドメインの語彙

| 記号 | 定義 | Rust 表現 |
| --- | --- | --- |
| `Date` | `YYYY-MM-DD` の暦日。辞書順 = 日付順 | `String` |
| `Time` | `HH:MM`（正規化後は必ず 2 桁時） | `String` |
| `Indent` | 行頭空白の幅。**バイト数**で数え、タブは 1 とする | `usize` |
| `Name` | Wiki リンクの正規化名（`.md` を落とし前後を trim した文字列） | `String` |
| `Tag` | `#` に続く空白と `#` を含まない文字列 | `String` |
| `Ref` | `Name` ∪ `Tag`。PJ への参照 | `String` |
| `Line` | 行番号。**`parser-core` は 0 始まり**、CLI の `toggle` 引数は 1 始まり | `usize` |

`Indent` の数え方は 2 箇所（正規表現の `^(\s*)` のキャプチャ長と `pj::indent_width`）で独立に実装されているため、**両者が同じ数え方であることが不変条件**である（`indent_width` は `line.len() - line.trim_start().len()` でバイト数を返す）。片方を文字数に変えるとタブ混在時に帰属判定がずれる。

## 3. 表層構文（行の文法）

解析は行単位で、行の種別は次の文法で決まる。`digit` は ASCII 数字、`sp` は空白 1 文字。

```ebnf
line       = fence | heading | task | log | time_memo | other ;

fence      = indent , ( "```" | "~~~" ) , … ;
heading    = "#" × 1..6 , sp+ , text ;
task       = indent , "-" , sp* , "[" , marker , "]" , sp* , text ;
marker     = " " | "x" | "-" ;
log        = indent , "-" , sp* , date , [ sp+ , time , [ "-" , time ] ] , ":" , sp* , text ;
time_memo  = "-" , sp , time , ":" , sp , text ;          (* インデント不可・トップレベルのみ *)

date       = digit × 4 , "-" , digit × 2 , "-" , digit × 2 ;
time       = digit × 1..2 , ":" , digit × 2 ;
indent     = { " " | "\t" } ;
```

構文上の要点:

- `task` の `marker` は 1 文字の `[ x-]` に限る。したがって `- [[ノート名]]` は `[` の次が `[` なので `task` に**一致しない**。これは偶然ではなく要求（requirements.md 2.3）であり、正規表現を緩めてはならない。
- `log` の時刻部は省略可能で、`log` は「時刻なしログ」と「時刻付きログ」を包含する。実働判定（§5.5）だけが `timed_log`（時刻部を必須にした狭い文法）を使う。
- `time_memo` は行頭に空白を許さない。ジャーナルのトップレベル時刻メモだけを拾うためで、タスク配下のログと衝突させないための構文的な区別である。
- `fence` は開始・終了を区別せず、一致するたびにフラグを反転させる。言語指定付きの開始行（```` ```rust ````）も同じ規則で拾える。フェンス内の行はすべての解析から除外される。

## 4. 走査のセマンティクス（タスク文脈）

すべての行走査は次の状態機械で表せる。

```
State = { in_code : bool
        , current : Option<TaskCtx>
        , heads   : Vec<String>        -- parse_all_dates_internal のみ
        }
TaskCtx = { indent : Indent, status : TaskStatus, text : String, line : Line, context : Vec<String> }
```

遷移規則（上から順に最初に一致したものを適用する）:

| # | 入力行 | 遷移 |
| --- | --- | --- |
| R1 | `fence` | `in_code ← ¬in_code`、出力なし |
| R2 | `in_code = true` | 無視 |
| R3 | `heading(level, t)` | `heads ← heads[0..level-1] ++ [t]`（`current` は保持） |
| R4 | `task(i, m, t)` | `current ← Some(TaskCtx{ indent: i, .. })` |
| R5 | `log(i, d, s)` かつ `current = Some(c)` かつ `i > c.indent` | 出力を 1 件生成 |
| R6 | その他 | `current` を保持（弱い帰属） / 条件付きで `current ← None`（強い帰属） |

**帰属規則が 2 種類ある**ことが、このドメインで最も間違えやすい点である。

- **弱い帰属**（`parse_tasks_internal` / `parse_all_dates_internal` / `parse_schedule_internal`）: ログはインデントが厳密に深いという条件だけで直前のタスクに帰属する。間に見出しや段落が挟まっても文脈は閉じない。タスクとログが素直に隣接する通常のノートを対象にした緩い規則。
- **強い帰属**（`pj::journal_work`）: 上に加えて、**空でない行のインデントがタスク以下になった時点で `current ← None`**。空行だけでは閉じない。ジャーナルは `## 今日の候補` に PJ を並べ、別の場所に無関係な時刻メモを書く形が普通なので、弱い帰属のままだと「候補に載せただけ」が実働に化ける（requirements.md 6.1）。

`≥` ではなく `>`（厳密大なり）である点も規則として固定である。同じインデントの `- 2026-08-02: ...` はタスクの兄弟であってログではない。

## 5. Rust ドメインモデル

### 5.1 状態の代数

```rust
enum TaskStatus  { Incomplete, Completed, Cancelled }   // parser-core
enum ProjectStatus { Active, Someday, Done }            // parser-core（front matter）
enum PjHealth    { Ok, Unclarified, NoNext }            // parser-core::pj（導出値）
```

`TaskStatus` は `marker` との全単射である。

```
' ' ↔ Incomplete    'x' ↔ Completed    '-' ↔ Cancelled
```

`TaskStatus::from_marker` は全域関数として書かれている（未知の文字は `Incomplete`）が、呼び出し側は必ず `[ x-]` にキャプチャされた 1 文字を渡すため、実際に到達するのは上の 3 対応だけである。

3 状態は「対応の要否」と「完了したか」という 2 つの軸を 1 つの型に畳んだもので、集計の際は軸ごとに使い分ける。

| 用途 | Incomplete | Completed | Cancelled |
| --- | --- | --- | --- |
| 進捗の分母（`total_count`） | 含む | 含む | **含まない**（中立） |
| 進捗の分子（`completed_count`） | — | 含む | — |
| 今日以外の日付グループ | 表示 | 非表示 | 非表示 |
| 実働判定（§5.5） | — | 実働 | **実働でない** |

### 5.2 タスク（`parser-core`）

```rust
struct ParsedTask {                    // parse_tasks_internal の出力（日付を指定して抽出）
    status: TaskStatus, text: String, line: Line, log: String,
}

struct ParsedTaskWithDate {            // parse_all_dates_internal の出力
    status: TaskStatus, text: String, line: Line, log: String,
    date: String,                      // ← 本来は Option<Date>
    context: Vec<String>,              // 祖先見出しのスタック
}
```

`ParsedTaskWithDate` は「タスク × その日のログ」の直積であって、タスクそのものではない。1 つのタスク行が n 件のログを持てば n 個の値が生成される。**ログを 1 件も持たないタスクだけが `date = ""` の値を 1 個生成する。** 本来の代数は

```rust
enum Dated { NoDate, On(Date) }
```

だが、WASM 境界を素直に通すために空文字列で符号化している（P5）。判別子は `date.is_empty()`。

### 5.3 ツリーとスケジュール（`parser-core`）

```rust
struct FileInput { file_name: String, file_uri: String, lines: Vec<String> }   // 唯一の入力型（Deserialize）

struct TreeTaskData  { status, text, body: String, meta: Option<String>, file_uri, line, log, date, context }
struct TreeFileGroup { file_name: String, file_uri: String, tasks: Vec<TreeTaskData> }
struct TreeDateGroup { date_key: String, label: String, is_today: bool,
                       file_groups: Vec<TreeFileGroup>, completed_count: usize, total_count: usize }
```

ツリーは `Date → FileName → Task` の 3 段のグルーピングであり、3 種類の日付グループの直和を 1 つの構造体に畳んでいる。

```
TreeDateGroup ≅ Today { 全ステータス, 進捗カウンタ }
              | Past(Date) { 未完了のみ, カウンタ 0 }
              | Undated     { 未完了のみ, カウンタ 0 }
```

判別は `(is_today, date_key.is_empty())` の組で行う。出力順は **Today → Past（日付降順）→ Undated** で固定する。

```rust
struct ScheduleEntry {
    task_text: String, task_line: Line, status: TaskStatus,
    log_text: String, log_line: Line,
    time: String, end_time: String,     // ← 空文字列は「時刻なし」
    file_uri: String,
}
```

`ScheduleEntry` も本来は直和である。

```rust
enum ScheduleItem {
    FromTaskLog { task: TaskRef, status: TaskStatus, log: String, time: Time, end: Option<Time> },
    FromTimeMemo { text: String, time: Time },          // ジャーナルのトップレベル時刻メモ
}
```

現在の符号化では時刻メモ由来の項目は `task_text = ""`, `end_time = ""`, `status = Incomplete`, `task_line = log_line` を満たす。判別子として `task_text.is_empty()` を使うが、**これは全域的ではない**（§9 W3）。

`file_uri` は `parse_schedule_internal` の時点では常に `""` で、`build_schedule_data_internal` が埋める。中間表現に対する事後条件と最終出力に対する事後条件が異なる唯一の箇所である。

### 5.4 PJ ノート（`parser-core::pj`）

```rust
struct FrontMatterParsed { project: Option<ProjectStatus>, repo: Option<String>, completed: Option<String> }

struct PjLogEntry { date: String, text: String }

struct PjNote {
    next_action:      Option<String>,   // 原文（メタデータ込み）
    next_action_body: Option<String>,   // メタデータを除いた本文
    next_action_meta: Option<String>,   // 「30分・重・@PC」
    next_action_ai:   bool,
    health:           PjHealth,
    logs:             Vec<PjLogEntry>,  // 日付降順
    backlog:          Vec<String>,
}
```

`PjNote` は **ノート本文だけから決まる情報**という境界を持つ。git・ファイルシステムに依存する値（最終更新日・リポジトリの状態・ジャーナルでの言及）は一切含めない。この境界が P1・P2 を型として表している。

セクションは `#` または `##` の見出しで切り、`###` 以降は直前のセクションに含める（`## オープンタスク` の中を `###` でグループ分けしてよい、という規約のため）。記法の割り当ては次のとおりで、**チェックボックスを持つのは `## 次の予定` だけ**という制約が、`taski list` に流れ込む PJ 由来タスクを active な PJ あたり高々 1 件に抑えている。

| セクション | 記法 | 抽出関数 | 対応フィールド |
| --- | --- | --- | --- |
| `## 次の予定` | `- [ ]` | `extract_next_action` | `next_action*`, `health` |
| `## ログ` | `- YYYY-MM-DD: …` | `extract_logs` | `logs` |
| `## オープンタスク` | `- …`（チェックボックスなし） | `extract_backlog` | `backlog` |

`extract_next_action` が採るのは**マーカーが `" "` で本文が空でない最初の行**だけである。`- [x]` / `- [-]` しか無い（＝終わったが次を決めていない）状態も、セクションごと無い状態も、等しく `next_action = None` すなわち `health = NoNext` に落ちる。

判断メタデータの分離は次の述語で定義される。行末の括弧の中身を `・, 、 ，` で分割し、**いずれか 1 要素**が下の書式に一致するときだけメタデータと見なす。

```
is_meta_element(e) ⟺ e = "軽" ∨ e = "重"
                    ∨ (e[0] = '@' ∧ |e| > 1)
                    ∨ e ~ /^\d+\s*(分|時間)$/
                    ∨ e ~ /^(締切)?\d{1,2}-\d{1,2}$/
```

「いずれか 1 要素」で足りるとしているのは、`（45分・重）` のように一部だけ書く運用を許すため。逆に全要素を要求すると通常の注記より先に本物のメタデータを落とす。`（仮）` や `（髪・服・顔）` はどの要素も一致しないので分離されない。

### 5.5 参照と実働（`parser-core::wiki_link` / `pj`）

```rust
struct WikiLinkMatch  { name: String, start: usize, end: usize }   // start/end は元テキストのバイト位置
struct NormalizedName { name: String, is_journal: bool }
struct JournalWork    { date: String, refs: Vec<String> }
```

参照の抽出は 1 本に統一する（P4）。

```
collect_refs(t) = { normalize(m.name) | m ∈ parse_wiki_links(t) } ∪ extract_tags(t)
```

実働の定義は次の述語である。ジャーナル 1 日分の行列 `L` とそのファイル日付 `d₀` に対し、

```
journal_work(L, d₀) = { (d₀, R) | 完了タスク行 T ∈ L, R = collect_refs(text(T)) ≠ ∅, status(T) = Completed }
                    ∪ { (d,  R) | タスク行 T ∈ L, R = collect_refs(text(T)) ≠ ∅,
                                   時刻付きログ行 G が T の配下（強い帰属）, d = date(G) }
```

`Cancelled`（着手せず）は実働に含めない。時刻の**ない**ログも含めない。ジャーナルでは時刻なしログが予定・メモとしても書かれるため、含めると言及と区別がつかなくなる。

### 5.6 PJ 集約（`cli::pj`）

`PjProject` は `PjNote`（純粋）と外部世界の観測（git・ジャーナル・ファイルシステム）の合成である。フィールドは由来ごとに 5 つの層に分かれる。

```rust
struct PjProject {
    // (a) ノート本文由来 — PjNote / front matter をそのまま持ち上げる
    name, path, status, repo, completed,
    next_action, next_action_body, next_action_meta, next_action_ai, health,
    backlog, backlog_count,
    logs, log_last,                                 // logs は基準日より後を落としたあとの列

    // (b) ファイルシステム由来
    repo_abs: Option<String>,                       // ~ 展開 + シンボリックリンク解決済み絶対パス

    // (c) git 由来
    updated: Option<String>, repo_last: Option<String>,
    has_remote: Option<bool>, ahead_count: Option<usize>,
    unreported: bool, unreported_count: usize,

    // (d) ジャーナル由来
    journal_last: Option<String>, journal_work_last: Option<String>,

    // (e) (a)〜(d) の日付と today の差として導出
    stale_days, log_days, repo_days, journal_days, journal_work_days: Option<i64>,
}
```

`Option` の意味は層ごとに一貫させる。**`None` は「観測できなかった」であって「0」でも「false」でもない。** とくに `has_remote`:

| `has_remote` | 意味 |
| --- | --- |
| `None` | `repo:` が無い / パスが存在しない / git リポジトリでない — **バックアップの有無を論じられない** |
| `Some(false)` | git リポジトリだが remote が未設定 — バックアップ無し |
| `Some(true)` | remote あり |

`None` と `Some(false)` を潰すと、`repo:` を持たないノート（そもそもリポジトリを伴わない PJ）が「バックアップ無し」として数えられてしまう。

日数フィールドはすべて `Option<i64>` で、元の日付が `None` なら日数も `None`。基準日より後の観測はあらかじめ捨てているので、値があるときは必ず `≥ 0`（§6 I-16）。

出力全体は次のとおり。

```rust
struct PjOutput { generated: String, fetched: bool, fetch_failed: Vec<String>, projects: Vec<PjProject> }
```

`fetch_failed` が空でないとき、そこに挙がったリポジトリを持つ PJ の `repo_last` / `ahead_count` は信用できない。**この不確かさを `PjProject` 側に潰さず、出力の最上位に残す**のは、利用側が「古いかもしれない値」と「観測できなかった値（`None`）」を区別できるようにするため。

## 6. 不変条件

出力に対して常に成り立つべき述語を列挙する。番号は他ドキュメントやコメントから参照するためのもの。

### タスク・ツリー

- **I-1** ログの帰属はインデント厳密大なり: 出力される全ログについて `indent(log) > indent(task)`。
- **I-2** 1 タスク行 `T` が生成する `ParsedTaskWithDate` は、`T` が持つログ数 `n` に対し `n ≥ 1` なら `n` 個（すべて `date ≠ ""`）、`n = 0` なら 1 個（`date = "" ∧ log = ""`）。両者は排他。
- **I-3** `TreeTaskData` の `body` / `meta` は `split_decision_meta(text)` に一致する。すなわち `meta.is_some() ⟺ split_decision_meta(text).is_some()`、かつ `meta = None ⟹ body = text`。
- **I-4** 今日以外の日付グループ: `¬is_today ⟹ (∀t. t.status = Incomplete) ∧ completed_count = 0 ∧ total_count = 0`。
- **I-5** 進捗カウンタ（今日のみ）: `total_count = #{t | t.status ≠ Cancelled}`、`completed_count = #{t | t.status = Completed}`。したがって `0 ≤ completed_count ≤ total_count`。
- **I-6** 空グループを出力しない: 出力される `TreeDateGroup` は `file_groups ≠ []`、各 `TreeFileGroup` は `tasks ≠ []`。
- **I-7** グループ順序: `Today` が存在すれば先頭、続いて `date_key` の降順、`Undated` が最後。

### スケジュール

- **I-8** 時刻の正規化: `time ≠ "" ⟹ |time| = 5 ∧ time ~ /^\d{2}:\d{2}$/`。`end_time` も同様。
- **I-9** 並び順: `time = ""` の項目はすべて末尾に来る。それ以外は `time` の辞書順（= 時刻順、I-8 が前提）。
- **I-10** `parse_schedule_internal` 単体の出力は常に `file_uri = ""` で、`build_schedule_data_internal` が由来した `FileInput` の `file_uri` で上書きする。中間表現と最終出力で事後条件が異なるのはここだけ。

### PJ ノート

- **I-11** health は導出値であり独立に設定されない:

  ```
  health = NoNext      ⟺ next_action = None
  health = Unclarified ⟺ next_action ≠ None ∧ next_action_meta = None
  health = Ok          ⟺ next_action ≠ None ∧ next_action_meta ≠ None
  ```

- **I-12** `next_action_body.is_some() ⟺ next_action.is_some()`。
- **I-13** `next_action_ai ⟹ next_action_meta.is_some()`。
- **I-14** `logs` は `date` の降順。同日は記載順を保つ（安定ソート）。`log_last = logs.first().date`。
- **I-15** `backlog` の各要素は非空で、タスク行でもログ行でもない箇条書き由来。

### PJ 集約

- **I-16** 観測由来の日付は基準日を越えない: `updated` / `log_last` / `repo_last` / `journal_last` / `journal_work_last` と `logs` の各要素、および `ahead_count` の母数は、`≤ today` のものだけから作る。したがって対応する `*_days` はすべて `≥ 0`。
  **巻き戻るのはこの観測由来のフィールドだけ**であり、`next_action` / `health` / `backlog` / `status` / `completed` はノートの現在の内容をそのまま出す（過去のノート内容を git から復元することはしない）。front matter の `completed:` が `today` より後になることは構文上ありうる。`--today` を渡した出力は「その日時点のスナップショット」ではない。
- **I-17** 実働は言及の部分集合: `journal_work_last.is_some() ⟹ journal_last.is_some()`。両者は同じ `collect_refs` を使うため成立する。日付についても、実働を検出したジャーナルファイルは同じ参照を含むので `journal_last ≥ (その実働のファイル日付)`（例外は §9 W5）。
- **I-18** 未反映の整合: `unreported ⟺ unreported_count > 0`。かつ

  ```
  unreported ⟺ (repo_last ≠ None ∧ log_last = None) ∨ (repo_last > log_last)
  ```

  比較は厳密大なりなので `log_last` 当日のコミットは反映済みと見なす。フラグと件数は同一のクエリ結果から作る（別々に数えると author date / committer date の基準差で食い違う）。
- **I-19** `ahead_count.is_some() ⟺ has_remote = Some(true)`。
- **I-20** 走査順に依存する集約をしない: `repo_last = max(commit_dates)`、ジャーナルの実働日も `max`。「新しい順に走査して最初に見つかったもの」ではない。走査順と日付順が一致しない書き方（前日ぶんの記録を今日のジャーナルに書く、rebase でコミット日と author date がずれる）が普通に起きるため。
  例外は `updated`（`note_last_updated`）で、こちらは `git log --name-only` の出力順で最初に現れた日付を採る。走査するのが taski リポジトリ 1 つで、rebase の頻度も低いという前提に寄りかかっている箇所であり、他所からのコミットが混ざるようになったら `max` に揃える必要がある。
- **I-21** 並び順（`table` / `json` 共通）: `(unreported ? 0 : 1, -log_days, health_rank, name)` の昇順。`log_days = None` は最も古い扱い。`health_rank` は `NoNext < Unclarified < Ok`。

### 構造化出力の契約

- **I-22** `--format json|yaml` を持つサブコマンドは、該当 0 件でも空配列を出力し終了コード 0 を返す。フォーマットの解釈は集計より前に済ませる（集計を先にすると 0 件時の早期 return がこの契約を破る）。

## 7. 写像の一覧

`parser-core`（純粋・全域）:

| 関数 | 型 | 備考 |
| --- | --- | --- |
| `parse_tasks_internal` | `(&[String], &str) -> Vec<ParsedTask>` | 指定日付のログを持つタスクのみ |
| `parse_all_dates_internal` | `&[String] -> Vec<ParsedTaskWithDate>` | I-2 |
| `build_tree_data_internal` | `(Vec<FileInput>, &str) -> Vec<TreeDateGroup>` | I-4〜I-7 |
| `parse_schedule_internal` | `(&[String], &str) -> Vec<ScheduleEntry>` | `file_uri = ""` |
| `build_schedule_data_internal` | `(Vec<FileInput>, &str) -> Vec<ScheduleEntry>` | I-8〜I-10 |
| `parse_front_matter` | `&[String] -> Option<FrontMatterParsed>` | `---` で始まらない / 閉じない / YAML 不正なら `None` |
| `extract_tags` | `&str -> Vec<String>` | 重複を除かない（出現順） |
| `extract_file_tags` | `(&[String], &str) -> Vec<String>` | `project: active` のときだけ 1 要素、他は空 |
| `pj::split_decision_meta` | `&str -> Option<(String, String)>` | §5.4 の述語 |
| `pj::parse_pj_note` | `&[String] -> PjNote` | I-11〜I-15 |
| `pj::collect_refs` | `&str -> Vec<String>` | 言及・実働で共有（P4） |
| `pj::journal_work` | `(&[String], &str) -> Vec<JournalWork>` | 強い帰属（§4） |
| `wiki_link::parse_wiki_links` | `&str -> Vec<WikiLinkMatch>` | 表示名付き（`[[名前 \| 表示]]`）は対象外 |
| `wiki_link::normalize_wiki_name` | `&str -> NormalizedName` | trim + `.md` 除去 + 日付判定 |
| `wiki_link::resolve_wiki_link` | `(&str, &[PathBuf]) -> Option<PathBuf>` | 候補列の**先頭一致**。優先順位は呼び出し側が候補の並びで表現する |
| `wiki_link::wiki_link_create_path` | `(&str, bool, &Path) -> PathBuf` | **事前条件あり**（下記） |

**型に現れない事前条件**: `wiki_link_create_path(name, is_journal = true, _)` は `name` が `^\d{4}-\d{2}-\d{2}$` に一致することを要求する（`name[0..4]` / `name[5..7]` でスライスするため）。実際の呼び出しは必ず `normalize_wiki_name` の `is_journal` をそのまま渡すので条件は満たされるが、この関数だけを直接呼ぶ場合は成り立たせる責任が呼び出し側にある。真の型は `(JournalName | NoteName, &Path) -> PathBuf` である。

`cli::pj`（副作用あり）:

| 関数 | 副作用 | 備考 |
| --- | --- | --- |
| `collect_projects` | fs 走査・git・ネットワーク | `note/*.md` → `Vec<PjProject>` |
| `note_last_updated` | `git log --name-only`（1 回） | PJ 数に比例した git 起動を避ける |
| `journal_dates` | ジャーナル走査 | 言及と実働を 1 回の走査で集める |
| `repo_info` | `git log` / `git remote` | I-18・I-19 を 1 回のクエリ結果から作る |
| `fetch_repos` | `git fetch`（並列 8 本） | 失敗しても集計を続行し、失敗を `fetch_failed` で返す |

## 8. 状態遷移

タスクの状態はトグルで巡回する。VS Code の `taski.toggleTask` と CLI の `taski toggle` は同じ巡回を実装する。

```
   [ ] ──→ [x] ──→ [-] ──┐
    ↑                     │
    └─────────────────────┘
```

`PjHealth` は遷移ではなく格子である。`NoNext ⊑ Unclarified ⊑ Ok` は「ノートに書かれた情報量」の順序であり、I-11 のとおり `(next_action, next_action_meta)` の有無から毎回計算される。状態として保持されないので、ノートを書き換えれば次の実行で必ず追随する。

## 9. 型に表れていない前提（既知の弱さ）

現在のモデルが型で守れていない点を明示する。ここに挙げたものは「直すべき」ではなく「テストと規約で守る」と決めた箇所である。改めるときは §10 の制約に従う。

- **W1. 日付・時刻が `String`。** `Date` / `Time` の newtype はなく、`YYYY-MM-DD` の辞書順が日付順と一致する性質に依存して文字列のまま比較・フィルタしている（`is_future`, `max`, ソート）。不正な日付（`2026-13-45`）は構文上は通り、日数計算（`days_between` = `chrono` によるパース）でだけ `None` に落ちる。
- **W2. 「無し」の表現が 2 通りある。** `parser-core` の境界型では空文字列（`ParsedTaskWithDate::date`, `TreeDateGroup::date_key`, `ScheduleEntry::time`）、`cli` の出力型では `Option`。境界を越える型は前者、越えない型は後者、という規約で使い分ける。
- **W3. `ScheduleEntry` の判別子が全域でない。** 時刻メモの判別に `task_text.is_empty()` を使うが、本文が空のタスク（`- [ ]` の直後に何も書かず、配下に日付ログを置いた場合）も同じ形になる。実害が小さいので許容している。
- **W4. 行番号の基点が層で違う。** `parser-core` は 0 始まり、CLI の `toggle <file> <line>` は 1 始まり。変換は CLI の引数処理でのみ行う。
- **W5. `repo_last` と基準日のタイムゾーン差。** `%ad`（author date）はコミット自身のタイムゾーンで描画されるため、自分より東のタイムゾーンで作られたコミットはローカルの「今日」より 1 日先の日付を持ちうる。それが未来として除外されると `repo_last` が 1 つ古いコミットに巻き戻り、`unreported` が false に倒れる。同様に、ジャーナルの時刻付きログが自分のファイル日付より後の日付を持つ場合、I-17 の日付についての含意（`journal_work_last ≤ journal_last`）は成り立たない。個人利用では発生頻度が低いので許容している。
- **W6. `PjProject.status` は `String`。** `parser-core` 側は `ProjectStatus` の列挙だが、JSON の表現を固定するため出力型では文字列に落としている。追加時は `status_label` と `parse_status_filter` の両方を更新する必要がある（型では強制されない）。
- **W7. JSON のキー命名が層で違う。** `parser-core` の境界型は `#[serde(rename_all = "camelCase")]` を持つ（TypeScript 側の慣習に合わせるため）が、`cli::pj` の出力型は指定なし＝ snake_case である。結果として `taski list --format json` は `fileUri` / `dateKey`、`taski pj --format json` は `next_action` / `log_last` を出す。同一 CLI の中で不揃いだが、どちらも既に利用側の契約になっているので揃えない。`PjHealth` だけは kebab-case 指定で `no-next` / `unclarified` / `ok` を出す。

## 10. 拡張の指針

- **新しい事実を足すときは `PjProject` に、判断を足すときは利用側に。** 「機械的に決まるか」が判定基準（P3）。`health` を増やしたくなったら、まずそれが `PjNote` のフィールドから決定的に導けるかを確かめる。導けないなら skill 側に置く。
- **`parser-core` に `std::fs` / `std::process` / 現在時刻を持ち込まない。** 基準日が要るなら引数で受ける。WASM ターゲットでリンクできなくなるという実際的な制約でもある。
- **境界型に newtype を入れるなら `#[serde(transparent)]` を付ける。** JSON / WASM の表現を変えずに型だけ強くできる。W1 を直すならこの形になる。逆に、表現が変わる変更（`String` → タグ付き列挙）は VS Code 拡張と CLI の利用側の両方を同時に直す必要がある。
- **同じ概念の抽出関数を増やさない（P4）。** 参照の抽出・メタデータの分離・インデント幅の算出は既存の関数を呼ぶ。新しく書きたくなったら、それは既存関数の引数を増やすべき場面である。
- **不変条件を足したらテストも `parser-core` 側に足す。** 純粋関数なので `#[cfg(test)] mod tests` で完結する。git やファイルシステムが絡む不変条件（I-16〜I-21）だけが `cli/tests/pj_cli.rs` の担当。
