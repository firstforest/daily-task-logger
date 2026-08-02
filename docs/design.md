# taski 設計 — 現状の実装

本ドキュメントは taski の実装を形式的に記述する。

- **[requirements.md](requirements.md)** — *何を* 満たすか（要求）
- **[architecture.md](architecture.md)** — *どこに* 置くか（クレート構成・パイプライン・ビルド）
- **[domain.md](domain.md)** — *何が何であるか*（ドメインモデル。目標形）
- **[syntax.md](syntax.md)** — *どう書くか*（記法。文書・行・行内）
- **本ドキュメント** — *どう表しているか*（型・走査・不変条件・写像）

要求の根拠（なぜその仕様なのか）は requirements.md 側に、ドメインの目標形は domain.md 側に、記法の規範は syntax.md 側にある。ここでは要求と概念を型と述語に落とした結果だけを扱い、根拠と書式は重複させない。

**本ドキュメントは現状を書く。** 「ドメインに何があり、どう関係するか」は domain.md が持ち、ここには実装されているものだけを置く。

| 章 | 立場 |
| --- | --- |
| §1 設計原則 | 実装が従う規律（P-*） |
| §2〜§8 | **現状。** 実装されている語彙・走査・型・不変条件・写像（記法そのものは syntax.md） |
| §9 既知の弱さ | 型で守れていないが、そう決めたもの（W-*） |
| §10 移行課題 | domain.md の概念とずれているので、いずれ直すもの（G-*） |
| §11 拡張の指針 | 触るときの制約 |

domain.md と食い違う箇所には、すべて §10 に対応する G-* がある。

## 1. 設計原則

**P1. 解析は全域かつ純粋な写像である。**
`parser-core` は入力 `Vec<String>`（行の列）から出力データ構造への写像だけを担う。ファイルシステム・git・時刻・環境変数に触れない。任意の入力に対して停止し、パニックせず、値を返す（例外は §7 の事前条件のみ）。これにより VS Code 拡張（WASM 経由）と CLI（直接リンク）で同じ入力から同じ出力が出ることが型レベルで保証される。

**P2. 副作用は `cli` に閉じる。**
git の呼び出し・ディレクトリ走査・並列 fetch・現在日付の取得は `cli/src/pj.rs` に置く。`parser-core` の関数は「基準日」「ファイルの日付」を **引数で受け取る**（`journal_work(lines, file_date)`, `build_tree_data_internal(files, today_str)`）。テストが時刻に依存しないのはこの分割の帰結である。

**P3. 事実と判断を型で分ける。**
`PjProject` が持つのは機械的に決まる事実（日付・件数・有無）だけで、「着手すべきか」「粒度が粗いか」は含めない。唯一の例外が `PjHealth` で、これは I-11 のとおり他フィールドから決定的に導出される要約であり、独立した判断ではない。

**P4. 同じ概念は 1 つの関数から出す。**
参照の抽出（`collect_refs`）、判断メタデータの分離（`split_decision_meta`）は言及側・実働側・`list` 側で同一の関数を共有する。片方だけが表記の揺れを拾うと、I-17 のような不変条件が壊れる。

**P5. 境界（WASM / JSON）を通る型は素直な表現に寄せる。**
`serde` の既定表現で往復できることを優先し、newtype やタグ付き列挙を境界型には持ち込まない。その代償として本来は直和である概念が「フラットな構造体＋空文字列の判別子」に潰れている箇所がある（§5.3・§9）。潰した箇所は必ず本ドキュメントで代数的な形を併記する。

## 2. ドメインの語彙

domain.md の語と、現状の Rust 表現の対応表である。`Rust 表現` が `（なし）` の行は型として存在しない概念で、それぞれ §10 に対応する G-* がある。

| 記号 | 定義 | Rust 表現 |
| --- | --- | --- |
| `Date` | `YYYY-MM-DD` の暦日。辞書順 = 日付順 | `String` |
| `Time` | `HH:MM`（正規化後は必ず 2 桁時） | `String` |
| `When` | `Day(Date)` \| `Moment(Date, Time)`。domain.md §1 の目標であり、現状は `date` + `Option<Time>` の 2 フィールドに潰れている（G-11） | （なし） |
| `Duration` | 長さ。`45分` / `2時間` | （なし。`（45分）` は `meta` 文字列の一部） |
| `Schedule` | `When` と `Duration` の組。少なくとも一方を持つ | （なし） |
| `Context` | `@` に続く文字列。着手の前提条件。閉じた集合ではない | （なし。`meta` 文字列の一部） |
| `Indent` | 行頭空白の幅。**バイト数**で数え、タブは 1 とする | `usize` |
| `Name` | Wiki リンクの正規化名（`.md` を落とし前後を trim した文字列） | `String` |
| `Tag` | `#` に続く空白と `#` を含まない文字列 | `String` |
| `Ref` | `Name` ∪ `Tag`。PJ への参照（照合は domain.md §4） | `String` |
| `PjId` | PJ の正規名 = ノートのファイル名。domain.md §4 の目標であり、現状は型として存在しない（G-2） | （なし） |
| `Line` | 行番号。**`parser-core` は 0 始まり**、CLI の `toggle` 引数は 1 始まり | `usize` |

`Indent` の数え方は 2 箇所（正規表現の `^(\s*)` のキャプチャ長と `pj::indent_width`）で独立に実装されているため、**両者が同じ数え方であることが不変条件**である（`indent_width` は `line.len() - line.trim_start().len()` でバイト数を返す）。片方を文字数に変えるとタブ混在時に帰属判定がずれる。

## 3. 表層構文の実装

**記法の規範は [syntax.md](syntax.md) にある。** 行の文法（EBNF）・正規化規則・誤検出しない書き方はそちらが持ち、ここでは走査（§4）と型（§5）に効く点だけを扱う。

実装は syntax.md §3 の文法を行単位の正規表現として持ち、判定は上から順に最初に一致したものを採る。種別と、走査・型への効き方の対応:

- **fence** — `in_code` を反転させるだけで `current` を捨てない（§4 R1）。フェンス内は全解析から除外。
- **heading** — `heads` スタックを更新する（§4 R3）。`ParsedTaskWithDate.context` の元になる。タスクの文脈は閉じない。
- **task** — `current` を差し替える（§4 R4）。`marker` と `TaskStatus` は全単射（§5.1）。
- **log** — `current` があり、インデントが厳密に深いときだけ出力を生む（§4 R5）。
- **time_memo** — `ScheduleEntry` を `task_text = ""` で生成する。この空文字列が判別子になっている（§5.3・W-3）。
- **bullet** — `pj::extract_backlog` でのみ使う（§5.4）。

型に効く要点:

- `log` の時刻部は省略可能で、「時刻なしログ」と「時刻付きログ」を包含する。実働判定（§5.5）だけが `timed_log_re`（時刻部を必須にした狭い文法）を使う。
- **時刻をキャプチャするのは `parse_schedule_internal` だけである。** 他の経路（`parse_tasks_internal` / `parse_all_dates_internal` / `pj::extract_logs`）は時刻部を非キャプチャで読み飛ばし、日付だけを取る（G-7）。
- `time_memo` の発火にはファイル内の日付見出し（`^#\s+(date)`）との一致が要る。日付見出しは走査中の文脈であって `Document.date` ではない（domain.md §1）。

**同じ文法の正規表現が複数箇所に独立して定義されている。** `lib.rs` に 3 組（`parse_tasks_internal` / `parse_all_dates_internal` / `parse_schedule_internal` がそれぞれローカルに構築）、`pj.rs` に 1 組（`OnceLock` で共有）、加えて実働判定用の `timed_log_re` がある。記法を変えるときは syntax.md を直したうえで、これらすべてを揃える必要がある（G-7）。

## 4. 走査のセマンティクス（タスク文脈）

すべての行走査は次の状態機械で表せる。

```
State = { in_code : bool
        , current : Option<TaskCtx>
        , heads   : Vec<String>        -- parse_all_dates_internal のみ
        }
TaskCtx = { indent : Indent, status : TaskStatus, text : String, line : Line, context : Vec<String> }
```

`in_code` と `current` は**直交する**。フェンス行は `in_code` を反転させるだけで `current` を捨てないので、タスクの配下にコードブロックを挟んでもログの帰属は切れない。

```mermaid
stateDiagram-v2
    state "行走査の状態（2 つの軸は直交する）" as Scan {
        [*] --> Body
        Body : in_code = false（本文・解析対象）
        Code : in_code = true（すべて無視）
        Body --> Code : fence 行
        Code --> Body : fence 行
        --
        [*] --> NoCtx
        NoCtx : current = None（タスク文脈なし）
        InCtx : current = Some（indent と本文を保持）
        NoCtx --> InCtx : task 行
        InCtx --> InCtx : task 行（文脈を差し替え）
        InCtx --> NoCtx : 参照を持たない task 行（強い帰属のみ）
        InCtx --> NoCtx : インデントがタスク以下の非空行（強い帰属のみ）
    }
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

## 5. Rust ドメインモデル（現状の型）

ここからは**実装されている型**を記述する。domain.md の概念（Document / Project / Task / Log / Observation）と 1 対 1 には対応していない。対応と差分は §10 にまとめてある。

型の全体像と、P1・P2 が引いている純粋／副作用の境界:

```mermaid
flowchart TB
    subgraph core["parser-core — 純粋・全域（fs / git / 時刻に触れない）"]
        L["行の列（lines）"]
        L --> A1["parse_all_dates_internal"] --> T1["ParsedTaskWithDate"] --> B1["build_tree_data_internal"] --> T5["TreeDateGroup<br/>→ TreeFileGroup<br/>→ TreeTaskData"]
        L --> A2["build_schedule_data_internal"] --> T2["ScheduleEntry"]
        L --> A5["parse_front_matter"] --> T6["FrontMatterParsed"]
        L --> A3["parse_pj_note"] --> T3["PjNote<br/>→ PjLogEntry"]
        L --> A4["journal_work"] --> T4["JournalWork"]
    end

    T5 --> UI["VS Code TreeView / taski list / taski schedule"]
    T2 --> UI
    T6 --> AGG["cli::pj の集約（fs 走査・git・ネットワーク）<br/>→ PjProject（§5.6）"]
    T3 --> AGG
    T4 --> AGG
```

`parser-core` 側の矢印はすべて**関数適用**であり、外部状態を経由しない。`cli` 側の箱だけがファイルシステム・git・ネットワークに触れる。基準日（`today`）もこの境界を越えて引数として渡される。

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

現在の符号化では時刻メモ由来の項目は `task_text = ""`, `end_time = ""`, `status = Incomplete`, `task_line = log_line` を満たす。判別子として `task_text.is_empty()` を使うが、**これは全域的ではない**（W-3）。

`file_uri` は `parse_schedule_internal` の時点では常に `""` で、`build_schedule_data_internal` が埋める。中間表現に対する事後条件と最終出力に対する事後条件が異なる唯一の箇所である。

§5.2〜§5.3 の型をまとめると次のとおり。実線のひし形は合成（所有）、破線の矢印は関数による導出を表す。多重度 `1..*` は I-6（空グループを出力しない）の帰結である。

```mermaid
classDiagram
    direction TB

    class TaskStatus {
        <<enumeration>>
        Incomplete
        Completed
        Cancelled
    }

    class FileInput {
        +String file_name
        +String file_uri
        +Vec~String~ lines
    }

    class ParsedTask {
        +TaskStatus status
        +String text
        +usize line
        +String log
    }

    class ParsedTaskWithDate {
        +TaskStatus status
        +String text
        +usize line
        +String log
        +String date
        +Vec~String~ context
    }

    class TreeDateGroup {
        +String date_key
        +String label
        +bool is_today
        +usize completed_count
        +usize total_count
    }

    class TreeFileGroup {
        +String file_name
        +String file_uri
    }

    class TreeTaskData {
        +TaskStatus status
        +String text
        +String body
        +Option~String~ meta
        +String file_uri
        +usize line
        +String log
        +String date
        +Vec~String~ context
    }

    class ScheduleEntry {
        +String task_text
        +usize task_line
        +TaskStatus status
        +String log_text
        +usize log_line
        +String time
        +String end_time
        +String file_uri
    }

    TreeDateGroup "1" *-- "1..*" TreeFileGroup : file_groups
    TreeFileGroup "1" *-- "1..*" TreeTaskData : tasks

    FileInput ..> ParsedTask : parse_tasks_internal
    FileInput ..> ParsedTaskWithDate : parse_all_dates_internal
    FileInput ..> ScheduleEntry : build_schedule_data_internal
    ParsedTaskWithDate ..> TreeTaskData : build_tree_data_internal

    ParsedTask --> TaskStatus
    ParsedTaskWithDate --> TaskStatus
    TreeTaskData --> TaskStatus
    ScheduleEntry --> TaskStatus
```

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

セクションの切り方と記法の割り当ては syntax.md §8.2 が規範である。実装では `split_sections` が見出し文字列をキーにした `HashMap` を作り、各抽出関数がそこから引く。

| セクション | 抽出関数 | 対応フィールド |
| --- | --- | --- |
| `## 次の予定` | `extract_next_action` | `next_action*`, `health` |
| `## ログ` | `extract_logs` | `logs` |
| `## オープンタスク` | `extract_backlog` | `backlog` |

**このセクション依存は G-10 の対象である。** 見出し文字列がドメインモデルに漏れており、目標形では射影がノート全体を見る。

`extract_next_action` が採るのは**マーカーが `" "` で本文が空でない最初の行**だけである。`- [x]` / `- [-]` しか無い（＝終わったが次を決めていない）状態も、セクションごと無い状態も、等しく `next_action = None` すなわち `health = NoNext` に落ちる。`health` はこの判定と判断メタデータの有無だけで決まる（I-11）。

```mermaid
flowchart TD
    S["次の予定セクション"] --> Q1{"マーカーが空白で<br/>本文が空でない行があるか"}
    Q1 -- "無い（セクション欠落・完了済みのみ）" --> H1["health = NoNext"]
    Q1 -- "ある" --> Q2{"行末に判断メタデータがあるか<br/>split_decision_meta"}
    Q2 -- "無い" --> H2["health = Unclarified"]
    Q2 -- "ある" --> H3["health = Ok"]
```

`next_action_ai` は同じメタデータに `@AI` が含まれるかどうかで別に決まる。メタデータが無ければ常に `false`（I-13）。

判断メタデータの書式（何がメタデータの要素と見なされるか）は syntax.md §5.3 が規範である。実装は次の 2 段構えになっている。

```
split_decision_meta(text) : Option<(body, meta)>
  = 行末括弧の正規表現でマッチし、中身を `・, 、 ，` で分割して
    is_meta_element がいずれか 1 要素に真なら Some((body.trim_end(), meta))
```

**判定が済むと分解結果を捨て、`meta` を文字列のまま返す。** その帰結として `next_action_ai` が同じ区切り文字でもう一度分割することになっており、P4 に反している（G-12）。`duration_re` / `deadline_re` は `is_meta_element` からしか呼ばれず、`45分` も `08-15` も値としては解釈されない。

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

```mermaid
flowchart TD
    T["journal のタスク行"] --> R{"collect_refs が空でないか"}
    R -- "空" --> X["実働でない<br/>（タスク文脈も閉じる）"]
    R -- "参照あり" --> C{"マーカー"}
    C -- "完了" --> WA["実働 = journal のファイル日付"]
    WA --> D{"より深いインデントの<br/>時刻付きログが配下にあるか"}
    C -- "未完了・見送り" --> D
    D -- "あり" --> WB["実働 = そのログ行の日付<br/>（複数ログなら複数件）"]
    D -- "なし" --> E["これ以上の実働なし"]
```

**2 つの枝は独立で、両方を満たすタスク行は 2 件の実働を生む。** マーカーが効くのは 1 つ目の枝だけである。したがって `Cancelled`（見送り）はそれ自体では実働にならないが、**配下に時刻付きログがあれば 2 つ目の枝で実働になる**。着手した時間の記録が残っている以上、チェックボックスの最終状態だけを見て捨てるのは誤りだからである（`test_journal_work_cancelled_task_with_timed_log_is_work`）。

時刻の**ない**ログはどちらの枝にも入らない。ジャーナルでは時刻なしログが予定・メモとしても書かれるため、含めると言及と区別がつかなくなる。

§5.4〜§5.5 の型をまとめると次のとおり。`PjNote` が `PjHealth` を、`FrontMatterParsed` が `ProjectStatus` を持つのに対し、`JournalWork` はどの型にも所有されない独立した観測値である（`cli` 側が日付の最大値を採るためだけに使う）。

```mermaid
classDiagram
    direction TB

    class ProjectStatus {
        <<enumeration>>
        Active
        Someday
        Done
    }

    class PjHealth {
        <<enumeration>>
        Ok
        Unclarified
        NoNext
    }

    class FrontMatterParsed {
        +Option~ProjectStatus~ project
        +Option~String~ repo
        +Option~String~ completed
    }

    class PjNote {
        +Option~String~ next_action
        +Option~String~ next_action_body
        +Option~String~ next_action_meta
        +bool next_action_ai
        +PjHealth health
        +Vec~String~ backlog
        +log_last() Option~str~
    }

    class PjLogEntry {
        +String date
        +String text
    }

    class JournalWork {
        +String date
        +Vec~String~ refs
    }

    class WikiLinkMatch {
        +String name
        +usize start
        +usize end
    }

    class NormalizedName {
        +String name
        +bool is_journal
    }

    PjNote "1" *-- "0..*" PjLogEntry : logs（日付降順）
    PjNote --> PjHealth : 導出値
    FrontMatterParsed --> ProjectStatus
    WikiLinkMatch ..> NormalizedName : normalize_wiki_name
    NormalizedName ..> JournalWork : collect_refs で refs を作る
```

### 5.6 PJ 集約（`cli::pj`）

`PjProject` は `PjNote`（純粋）と外部世界の観測（git・ジャーナル・ファイルシステム）の合成である。

```mermaid
flowchart LR
    N["note/PJ名.md"] --> FM["parse_front_matter<br/>project / repo / completed"]
    N --> PN["parse_pj_note<br/>next_action / logs / backlog"]
    JR["journal/年/月/日付.md"] --> JD["journal_dates<br/>言及 mention / 実働 work"]
    RP["repo: のリポジトリ"] --> FE["fetch_repos"] --> RI["repo_info<br/>repo_last / unreported / ahead"]
    RP --> AB["repo_abs_path<br/>~ 展開 + 実パス解決"]
    TK["taski リポジトリ"] --> NU["note_last_updated<br/>updated"]

    FM --> P["PjProject"]
    PN --> P
    JD --> P
    RI --> P
    AB --> P
    NU --> P
    P --> DY["stale_days / log_days / repo_days<br/>journal_days / journal_work_days<br/>= today − 各日付"]
```

フィールドは由来ごとに 5 つの層に分かれる。この層分けは出力の都合ではなく domain.md §2 の区別（a が Project の定義、b〜e が Observation）に対応している。

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

日数フィールドはすべて `Option<i64>` で、元の日付が `None` なら日数も `None`。基準日より後の観測はあらかじめ捨てているので、値があるときは必ず `≥ 0`（I-16）。

出力全体は次のとおり。

```rust
struct PjOutput { generated: String, fetched: bool, fetch_failed: Vec<String>, projects: Vec<PjProject> }
```

`fetch_failed` が空でないとき、そこに挙がったリポジトリを持つ PJ の `repo_last` / `ahead_count` は信用できない。**この不確かさを `PjProject` 側に潰さず、出力の最上位に残す**のは、利用側が「古いかもしれない値」と「観測できなかった値（`None`）」を区別できるようにするため。

クレート境界をまたぐ所有関係は次のとおり。`PjLogEntry` と `PjHealth` は `parser-core` の型のまま `cli` の出力に載る（`cli` 側で写し替えない）。

```mermaid
classDiagram
    direction LR

    class PjOutput {
        <<cli::pj>>
        +String generated
        +bool fetched
        +Vec~String~ fetch_failed
    }

    class PjProject {
        <<cli::pj>>
        a. ノート本文由来
        b. ファイルシステム由来
        c. git 由来
        d. ジャーナル由来
        e. today との差で導出
    }

    class PjNote {
        <<parser-core::pj>>
    }

    class FrontMatterParsed {
        <<parser-core>>
    }

    class PjLogEntry {
        <<parser-core::pj>>
    }

    class PjHealth {
        <<parser-core::pj>>
    }

    PjOutput "1" *-- "0..*" PjProject : projects
    PjNote "1" *-- "0..*" PjLogEntry : logs（切り詰め前）
    PjProject "1" *-- "0..3" PjLogEntry : logs（LOG_LIMIT = 3）
    PjProject --> PjHealth : health
    PjNote ..> PjProject : (a) 本文由来のフィールド
    FrontMatterParsed ..> PjProject : (a) status / repo / completed
```

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
- **I-17** 実働は言及の部分集合: `journal_work_last.is_some() ⟹ journal_last.is_some()`。両者は同じ `collect_refs` を使うため成立する。domain.md §5 の導出形ではこれは定理になり、不変条件として書く必要がなくなる（G-4）。日付についても、実働を検出したジャーナルファイルは同じ参照を含むので `journal_last ≥ (その実働のファイル日付)`（例外は W-5）。
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

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Incomplete
    Incomplete : Incomplete（マーカー = 半角空白）
    Completed : Completed（マーカー = x）
    Cancelled : Cancelled（マーカー = ハイフン）
    Incomplete --> Completed : toggle
    Completed --> Cancelled : toggle
    Cancelled --> Incomplete : toggle
```

3 状態の巡回なので、トグルは 3 回で恒等写像に戻る。マーカー以外（インデント・本文・行内の位置）は保存する。

`PjHealth` は遷移ではなく格子である。`NoNext ⊑ Unclarified ⊑ Ok` は「ノートに書かれた情報量」の順序であり、I-11 のとおり `(next_action, next_action_meta)` の有無から毎回計算される。状態として保持されないので、ノートを書き換えれば次の実行で必ず追随する。

## 9. 型に表れていない前提（既知の弱さ）

現在のモデルが型で守れていない点を明示する。ここに挙げたものは「直すべき」ではなく「テストと規約で守る」と決めた箇所である。改めるときは §11 の制約に従う。

- **W-1. 日付・時刻が `String`。** `Date` / `Time` の newtype はなく、`YYYY-MM-DD` の辞書順が日付順と一致する性質に依存して文字列のまま比較・フィルタしている（`is_future`, `max`, ソート）。不正な日付（`2026-13-45`）は構文上は通り、日数計算（`days_between` = `chrono` によるパース）でだけ `None` に落ちる。
- **W-2. 「無し」の表現が 2 通りある。** `parser-core` の境界型では空文字列（`ParsedTaskWithDate::date`, `TreeDateGroup::date_key`, `ScheduleEntry::time`）、`cli` の出力型では `Option`。境界を越える型は前者、越えない型は後者、という規約で使い分ける。
- **W-3. `ScheduleEntry` の判別子が全域でない。** 時刻メモの判別に `task_text.is_empty()` を使うが、本文が空のタスク（`- [ ]` の直後に何も書かず、配下に日付ログを置いた場合）も同じ形になる。実害が小さいので許容している。domain.md §1 の `attach(log) : Option<Task>` に直せば判別子が全域になり、この弱さは消える（G-7）。
- **W-4. 行番号の基点が層で違う。** `parser-core` は 0 始まり、CLI の `toggle <file> <line>` は 1 始まり。変換は CLI の引数処理でのみ行う。
- **W-5. `repo_last` と基準日のタイムゾーン差。** `%ad`（author date）はコミット自身のタイムゾーンで描画されるため、自分より東のタイムゾーンで作られたコミットはローカルの「今日」より 1 日先の日付を持ちうる。それが未来として除外されると `repo_last` が 1 つ古いコミットに巻き戻り、`unreported` が false に倒れる。同様に、ジャーナルの時刻付きログが自分のファイル日付より後の日付を持つ場合、I-17 の日付についての含意（`journal_work_last ≤ journal_last`）は成り立たない。個人利用では発生頻度が低いので許容している。
- **W-6. `PjProject.status` は `String`。** `parser-core` 側は `ProjectStatus` の列挙だが、JSON の表現を固定するため出力型では文字列に落としている。追加時は `status_label` と `parse_status_filter` の両方を更新する必要がある（型では強制されない）。
- **W-7. JSON のキー命名が層で違う。** `parser-core` の境界型は `#[serde(rename_all = "camelCase")]` を持つ（TypeScript 側の慣習に合わせるため）が、`cli::pj` の出力型は指定なし＝ snake_case である。結果として `taski list --format json` は `fileUri` / `dateKey`、`taski pj --format json` は `next_action` / `log_last` を出す。同一 CLI の中で不揃いだが、どちらも既に利用側の契約になっているので揃えない。`PjHealth` だけは kebab-case 指定で `no-next` / `unclarified` / `ok` を出す。
- **W-8. Project の同一性がパスに依存する。** `PjId` はノートのファイル名なので、リネームすると別の Project になる。ジャーナルに残った `[[旧名]]` は解決先を失い、その Project の言及・実働の履歴が途切れる（`journal_last` / `journal_work_last` が `null` に戻る）。front matter に安定 ID を持たせれば切り離せるが、参照側は `[[名前]]` のままなので、リンクの追随は別途必要になる。現状は「リネームしない・するなら参照も一括で書き換える」という運用で回避している。改めるなら §11 の制約に従う。

## 10. ドメインモデルとの差分（移行課題）

domain.md の目標形と現状の実装（§5〜§7）の差を列挙する。番号は着手順ではない。§9 の W-* が「型で守れていないが、そう決めた」ものであるのに対し、こちらは「概念とずれているので、いずれ直す」ものである。

概念と現状の型の対応:

| 概念（domain.md） | 現状の実装 | ずれ |
| --- | --- | --- |
| `Document` | `FileInput`（`file_uri` + `lines`）、または `&[String]` と別引数 | G-8 |
| `Project` 本体（`id` + `status`） | ファイル名 + `FrontMatterParsed.project`。`completed` が独立している | G-9 |
| Project の射影（`tasks` / `logs` / `health`） | `pj::PjNote`。セクション名で絞り込んでおり、`next_action` / `backlog` という概念に無いものを作っている | G-3・G-10 |
| 観測の構成（`repo`） | `FrontMatterParsed.repo`。定義と同じ層に置かれている | — |
| `Observation` | `cli::pj::PjProject` の b〜e 層（§5.6） | — |
| `Task` | `ParsedTask` / `ParsedTaskWithDate` / `ScheduleEntry` / `next_action: String` | G-3・G-7 |
| `Log` | `ParsedTaskWithDate.log` / `ScheduleEntry` / `pj::PjLogEntry` の 3 通り | G-7 |
| `attach(log)` | 型に無い。平坦化・空文字列判別・概念の欠落で表現している | G-7 |
| `When` | `date: String` + `time: Option<Time>` の 2 フィールドに潰れている | G-11 |
| `Duration` | 型に無い。`（45分）` は `meta` 文字列の一部、`10:00-11:00` は `end_time` | G-11 |
| `Task.when`（計画） | 存在しない。配下の Log が兼ねている | G-11 |
| `Schedule` / `Context` | 型に無い。`pj::split_decision_meta` が返す `meta` 文字列の一部。所要時間・締切は値として解釈すらされていない | G-12 |
| `Ref` | 型として存在しない（`String`。名前とタグが混ざった `Vec<String>`） | G-2 |
| `PjId` | 型として存在しない（`String`） | G-2 |
| 層 1 収容 | `FileInput.file_uri` + `ParsedTask.line` | G-8 |
| 層 2 参照 | `pj::collect_refs` | — |
| 層 3 解決 | `wiki_link::resolve_wiki_link` と `refs.contains` の 2 系統 | G-5・G-6 |
| 層 4 役割 | `parse_front_matter` + `cli::pj::collect_projects` | — |
| `projects(task)`（層 1〜4 の合成） | 合成として存在しない。`extract_file_tags` が層 1 の導出で層 4 を先取りしている | G-1 |
| `mention` / `work` | `cli::pj::journal_dates`（ジャーナルを直接読む） | G-4 |

**G-1 タグ導出が層をまたいで PJ の `status` を見ている。**

- 現状: `extract_file_tags` は「ファイル名をそのファイル内の全タスクのタグにする」導出、すなわち domain.md §3 の層 1・2 の話である。にもかかわらず `project: active` を条件にしており、層 4（Document → Project の役割）の情報を層 1 の導出が参照している。
- 目標: タグ導出を層 1・2 で完結させ、`status` に依存させない。絞り込みは表示側（タグ別ビュー・`list --tag`）の責務とする。
- 影響: `someday` / `done` の PJ ノートに書いたタスクにもファイル名タグが付く。出したくなければ表示側でフィルタする。requirements.md 2.2 の「`someday` / `done` では自動タグを付与しない」は、パーサーの要求ではなくビューの要求として書き直すことになる。

**`extract_file_tags` / `collect_refs` / `parse_pj_note` の 3 つは「同じ概念の 3 実装」ではない。** 順に層 1 のタグ導出・層 2 の参照抽出・PJ ノート内での役割であり、統合すべきものではない。`taski list` と `taski pj` が別世界を見ているように見えるのは、前者が層 1・2 で止まり、後者が層 3・4 まで辿るからである（domain.md §3）。この差は設計であって欠陥ではない。実際に直すべきなのは、上の層またぎ（G-1）と、層 3 が 2 系統あること（G-5）の 2 点だけである。

**G-2 `PjId` が無く、表記変換が 2 箇所に独立実装されている。**

- 現状: `name.replace(' ', "_")` が `cli::pj::journal_dates` と `parser_core::extract_file_tags` にある。逆変換は無く、照合は `refs.contains(name) || refs.contains(tag)` の両方試しで回避している。
- 目標: domain.md §4 の `match_key` を 1 箇所に置き、PJ ノートの `match_key` の一意性を不変条件にする。
- 影響: `在庫 管理.md` と `在庫_管理.md` が同居すると、現在は黙って両方に同じ言及が付く。一意性を課すとエラーとして表面化する。

**G-3 PJ ノートのタスクが Task 型になっていない。**

- 現状: `PjNote.next_action: Option<String>`。`## 次の予定` の 1 行を文字列として持つだけで、行番号もファイルも持たない。同じチェックボックス行なのに `ParsedTask` にはならず、`taski pj` の出力から元の行へ飛べない。ノート内の他のチェックボックス行はそもそも読まれない。
- 目標: domain.md §2 の `tasks(pj)`。ノート内のチェックボックス行をすべて Task として扱い、位置（`at`）を持たせる。
- 影響: `PjProject` の出力が「次の予定 1 行の文字列」から「未完了 Task の列（位置つき）」に変わる。VS Code 側から PJ のタスクへ遷移できるようになる。G-10 の一部として同時に行うことになる。

**G-4 観測が Task を経由していない。**

- 現状: `cli::pj::journal_dates` がジャーナルの本文を直接読んで言及と実働を集める。Task の集合を作らないので、I-17（実働 ⊆ 言及）は「両者が同じ `collect_refs` を呼ぶ」という規律でしか守られていない。
- 目標: domain.md §5 の導出に直す。I-17 が定理になり、不変条件として書く必要がなくなる。
- 影響: 素朴に置き換えると遅くなる。現状は「見つけたら打ち切り」の早期終了（`remaining_mention` / `remaining_work`）が効いていて、全ジャーナルを読み切らない。導出形にしてもこの枝刈りを保てる形にする必要がある。

**G-5 参照の解決が 2 系統ある。**

- 現状:

  | 用途 | 実装 | 探索範囲 |
  | --- | --- | --- |
  | `[[n]]` を開く | `wiki_link::resolve_wiki_link` | taski home 配下の**全 md**。パスのソート順で先頭一致 |
  | PJ を照合する | `refs.contains(name) \|\| refs.contains(tag)` | `note/` **直下のみ**。文字列一致 |

  同じ `[[在庫管理]]` が、開く時はファイル探索で、集計時は文字列一致で解決される。`note/sub/在庫管理.md` に `project: active` を書くと「開けるが PJ にならない」。
- 目標: domain.md §4 の `resolve` 1 本に集約する。
- 影響: PJ の探索範囲を広げるかは別の判断（G-6）。「解決関数を一致させること」と「範囲を決めること」を分けて扱う。

**G-6 走査範囲が非対称。**

- 現状: ジャーナルは再帰（`cli::pj::collect_journal_files`）、PJ ノートは直下のみ（`cli::pj::collect_note_files`）、`taski list` は taski home 全体を再帰（`main::collect_md_files`）。3 つの走査規則が別々に書かれている。
- 目標: Document 集合を作る関数を 1 つにし、用途ごとの絞り込みは述語で表す。
- 未決: PJ ノートを `note/**` に広げるか。広げると `note/archive/` のような置き場が PJ として拾われ、`status` による絞り込みと役割が重なる。

**G-7 ログの表現が 3 つあり、Task への帰属が型に無い。**

- 現状: `- 2026-08-01 10:00: 本文` という 1 行が、経路によって 3 通りに落ちる。

  | 型 | 日付 | 時刻 | 本文 |
  | --- | --- | --- | --- |
  | `ParsedTaskWithDate` | `date` フィールド | **捨てる**（正規表現が非キャプチャ） | `log` |
  | `ScheduleEntry` | （持たない） | `time` / `end_time` | `log_text`。タスクと平坦化されている |
  | `pj::PjLogEntry` | `date` | **捨てる** | `text` |

  同一の正規表現パターンが `lib.rs` と `pj.rs` に別々に定義されており、実働判定はさらに `timed_log_re`（時刻部を必須にした 4 本目）を引いている。

  さらに **Task への帰属が型に表れていない**。3 つの型がそれぞれ違う方法で誤魔化している。

  | 型 | 帰属の扱い |
  | --- | --- |
  | `ParsedTaskWithDate` | タスクとログを 1 構造体に平坦化し、帰属していることを前提にしている |
  | `ScheduleEntry` | 帰属の有無を `task_text.is_empty()` で判別する（全域でない — W-3） |
  | `pj::PjLogEntry` | 帰属の概念自体が無い（`## ログ` は元から帰属しないので困っていない） |
- 目標: domain.md §1 の `Log { at, when, duration, text }` 1 つと、関係 `attach(log) : Option<Task>`。時刻を `When` として構造で持てば実働判定は正規表現をもう 1 本引くのではなく「`log.when` が `Moment` か」になり、帰属を `Option` で持てば W-3 の判別子が全域になる。
- 影響: `ParsedTaskWithDate` は境界型なので JSON / WASM の表現が変わる。P5 と W-2 の制約下では、利用側（VS Code 拡張・CLI の両方）の同時修正が要る変更になる。

**G-8 Document がヘッダを持たず、呼び出し側が付随情報を渡している。**

- 現状: 解析関数は `&[String]` を受け取り、日付・パス・URI は別引数で渡される（`journal_work(lines, file_date)` / `build_tree_data_internal(files, today)`）。`FileInput` だけが `file_uri` と `lines` を束ねている。
- 目標: **P1 は変えない。** 純粋・全域である以上、日付やパスを引数で受けること自体は正しい。ただし「Document のヘッダ」を 1 つの型にまとめれば、呼び出し側が日付とパスを取り違える余地が減る（`journal_work(lines, file_date)` の `file_date` に `today` を渡す事故が型で防げる）。
- 影響: 小さい。`FileInput` の拡張で済み、P1 も P5 も壊さない。

**G-9 `completed` が `status` から独立している。**

- 現状: `FrontMatterParsed { project: Option<ProjectStatus>, completed: Option<String> }` で 2 つが独立したフィールドになっている。`project: active` かつ `completed: 2026-01-01` が表現でき、`PjProject` にもそのまま出る。I-16 が「front matter の `completed:` が `today` より後になることは構文上ありうる」と断っているのも、この独立性の帰結である。
- 目標: domain.md §2 のとおり `Status = Active | Someday | Done(Option<Date>)` に畳み、`completed` を `Done` の中にだけ置く。
- 影響: 小さい。front matter の 2 キー表記（`project:` / `completed:`）は変えず、`PjProject` の JSON も `status: String`（W-6）と `completed: Option<String>` のまま出せる。畳むのは `parse_front_matter` の戻り値から `collect_projects` までの区間に閉じるので、利用側の契約は変わらない。

**G-10 セクション名がドメインモデルに漏れている。**

- 現状: `parse_pj_note` は `split_sections` で `## 次の予定` / `## ログ` / `## オープンタスク` という 3 つの日本語見出しを探し、そこから `next_action` / `logs` / `backlog` を作る。`health` は `next_action` から導出される（I-11）。requirements.md 2.3 の「チェックボックスを持つのは `## 次の予定` だけ」という規約は、この構造を成り立たせるための書式の制約である。
- 目標: domain.md §2 のとおりセクションをドメインから外す。射影はノート全体から実体を拾い、`next_action` と `backlog` は概念から消す。`health` は `tasks(pj)` から再定義する。
- 影響: **本ドキュメントの移行課題の中で最も大きい。**

  | 影響 | 内容 |
  | --- | --- |
  | 書式の規約 | 「チェックボックスは `## 次の予定` だけ」が不要になる。PJ ノートが複数の Task を持てるようになり、それらは `taski list` の「日付なし」グループに出る。絞り込みはビュー側の責務とする（G-1 と同じ結論） |
  | 出力契約 | `taski pj` から `next_action` / `next_action_body` / `next_action_meta` / `next_action_ai` / `backlog` / `backlog_count` が消え、未完了 Task の列（位置つき）が入る。`cli/AGENTS.md` と利用側 skill の同時修正が要る |
  | 実装 | `split_sections` / `extract_next_action` / `extract_backlog` が不要になる。`extract_logs` は「帰属しない Log」の抽出に変わる |
  | 不変条件 | §6 の I-11 は `tasks(pj)` ベースに書き換え、I-12 / I-13 / I-15 は対象が消えるので削除になる |
  | 誤検出 | 説明文中に日付行（`2026-08-01: 締切` など）を書くとログとして拾われる。セクションによる隔離が無くなるため、`## ログ` の外に書いた記録も拾えるようになるのと表裏である |

  P3 の例外（`PjHealth`）は残る。基盤が `next_action` から `tasks(pj)` に移るだけで、「他フィールドから決定的に導出される要約」という性格は変わらない。

**G-11 Task が「いつ・どれくらい」を持たず、Log がそれを兼ねている。**

- 現状: Task は時刻を持たない。予定を入れるには配下に日付付きの行を書くしかなく、その行は Log としても解釈される。結果 1 本の行が 3 つの意味を背負う。

  | 用途 | 読み手 | 何を取るか |
  | --- | --- | --- |
  | スケジュールの枠（計画） | `parse_schedule_internal` | `time` / `end_time` |
  | 実績のテキスト | スケジュールグリッドの実績列 | `log_text` |
  | 実働の証拠 | `pj::journal_work` | 時刻の有無 |

  **計画と実績の対比が成立していない。** `src/schedulePanel.ts` は計画列にタスク本文、実績列にログ本文を出すが、**行が置かれる時刻はログ由来の 1 つだけ**である。「10:00-11:00 の予定が実際は 10:15-11:20 だった」というずれを表現できない。requirements.md 3.5 の「計画（plan）と実績（actual）を対比できる構成」は、データモデル上は満たされていない。

  所要時間も 2 箇所にある。判断メタデータの `（45分）` はスケジュールに使われず、ログの `10:00-11:00` は `list` に出ない。
- 目標: domain.md §1 の `Schedule`（`When` + `Duration`）。Task が自分の `schedule` を持ち、Log は起きたことの記録に徹する。`10:00-11:00` は `Moment(10:00)` + `Duration(60分)` の表記になり、`end_time` は導出値として消える。
- 影響:

  | 影響 | 内容 |
  | --- | --- |
  | 記法 | タスクの配下に置く属性行（`- 予定: 2026-08-01 10:00` / `- 所要: 45分`）に決まっている（syntax.md §10）。行末の括弧を拡張するのではなく置き換える。ログ行と同じ帰属規則に乗るので、走査（§4）に新しい規則は要らない |
  | 型 | `ScheduleEntry` の `time` / `end_time` が `When` + `Duration` に変わる。境界型なので JSON / WASM の表現が変わり、`schedulePanel.ts` の同時修正が要る（P5・W-2） |
  | スケジュール | グリッドが計画列と実績列に別々の時刻を持てるようになる。ずれの表示は新機能なので、UI の設計が要る |
  | 実働判定 | 「時刻の無いログは実働と見なさない」の根拠が消える（domain.md §5）。予定が Task 側に移れば Log は常に記録になるため。`Day` を実働に含めるかは**未決** |
  | 語彙 | `Time` の newtype 化（W-1）と同時にやると手戻りが少ない。`When` は `Date` / `Time` を包む型なので、先に文字列のまま `When` を作ると二度手間になる |

**G-12 `meta` が構造を持たない文字列である。**

- 現状: `split_decision_meta(text) -> Option<(body, meta)>` は括弧の中身を `・,、` で分割し、1 要素でも書式に一致すれば括弧全体を `meta` とする。**判定が済むと分解結果を捨てて文字列のまま返す。** その帰結が 3 つある。

  | 帰結 | 内容 |
  | --- | --- |
  | 再パース | `next_action_ai` は返ってきた `meta` を同じ区切り文字でもう一度分割して `@AI` を探している（`parser-core/src/pj.rs`）。P4（同じ概念は 1 つの関数から出す）に反する |
  | 契約が半端 | `taski list --format json` の `meta` は `"45分・重・@PC"` という文字列で、利用側が `45分` をパースし直す必要がある。requirements.md 6 の「利用側に同じ判定を再実装させないため」という意図を半分しか満たしていない |
  | health が粗い | `Unclarified` は「`meta` が無いこと」で決まるので、`（@PC）` だけ書いたタスクも `ok` になる。**何が決まっていないか**を言えない |
  | 値になっていない | `duration_re` / `deadline_re` は `is_meta_element` からしか呼ばれない。つまり `45分` も `08-15` も**「メタデータらしさ」の判定にしか使われず、値として解釈されていない**。`meta` 文字列に入って表示されるだけである |
- 目標: domain.md §1 のとおり `schedule` / `contexts` / `deadline` を Task の属性にする。行末の括弧は属性行（syntax.md §10）に置き換わり、`meta` という概念は消える。重さ（`軽` / `重`）は属性にしない。
- 影響:

  | 影響 | 内容 |
  | --- | --- |
  | 出力契約 | `taski list --format json` の `meta: String` が構造化フィールドに変わる。`body` はそのまま |
  | 冗長の解消 | `next_action_ai` が不要になる（`contexts` を見れば分かる）。G-10 で `next_action` 自体が消えるので二重に不要 |
  | 機能追加 | 所要時間と締切が初めて**値として**扱われる。整理ではなく機能の追加である |
  | 重さの廃止 | `軽` / `重` を認識しなくなるので、`（重）` としか書いていないタスクは括弧が本文に残る。既存のノートの一括修正が要る |
  | health | 「何が決まっていないか」を言えるようになる。ただし `Unclarified` の定義を変えるかは**未決**（重さが無くなるぶん、判定の材料は `schedule` / `contexts` / `deadline` に絞られる） |
  | 順序 | G-11 と同じ括弧を触る（`schedule` はどちらの課題にも現れる）ので、まとめて行うことになる |

  **記法は [syntax.md](syntax.md) §10 に定まっている。** タスクの配下に `- 予定: …` / `- 所要: …` / `- 締切: …` / `- 文脈: …` を置く形で、値はキーで区別されるため `split_decision_meta` のような「メタデータらしさ」の判定そのものが要らなくなる。実装するまでは現在の括弧の記法がそのまま有効である。

## 11. 拡張の指針

- **新しい事実を足すときは `PjProject` に、判断を足すときは利用側に。** 「機械的に決まるか」が判定基準（P3）。`health` を増やしたくなったら、まずそれが `PjNote` のフィールドから決定的に導けるかを確かめる。導けないなら skill 側に置く。
- **`parser-core` に `std::fs` / `std::process` / 現在時刻を持ち込まない。** 基準日が要るなら引数で受ける。WASM ターゲットでリンクできなくなるという実際的な制約でもある。
- **境界型に newtype を入れるなら `#[serde(transparent)]` を付ける。** JSON / WASM の表現を変えずに型だけ強くできる。W-1 を直すならこの形になる。逆に、表現が変わる変更（`String` → タグ付き列挙）は VS Code 拡張と CLI の利用側の両方を同時に直す必要がある。
- **同じ概念の抽出関数を増やさない（P4）。** 参照の抽出・メタデータの分離・インデント幅の算出は既存の関数を呼ぶ。新しく書きたくなったら、それは既存関数の引数を増やすべき場面である。
- **不変条件を足したらテストも `parser-core` 側に足す。** 純粋関数なので `#[cfg(test)] mod tests` で完結する。git やファイルシステムが絡む不変条件（I-16〜I-21）だけが `cli/tests/pj_cli.rs` の担当。
