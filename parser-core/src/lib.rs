pub mod pj;
pub mod wiki_link;

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

// === Task status ===

/// タスクの状態。
/// - `Incomplete`: `- [ ]` 未完了
/// - `Completed`: `- [x]` 完了
/// - `Cancelled`: `- [-]` 見送り（着手予定だったが着手せず、やらない or 別所へ繰り越し済み）
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Incomplete,
    Completed,
    Cancelled,
}

impl TaskStatus {
    /// チェックボックスのマーカー文字（` ` / `x` / `-`）から状態を判定する。
    fn from_marker(marker: &str) -> Self {
        match marker {
            "x" => TaskStatus::Completed,
            "-" => TaskStatus::Cancelled,
            _ => TaskStatus::Incomplete,
        }
    }
}

// === Internal types ===

struct CurrentTask {
    indent: usize,
    status: TaskStatus,
    text: String,
    line: usize,
    context: Vec<String>,
}

// === Output types ===

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTask {
    pub status: TaskStatus,
    pub text: String,
    pub line: usize,
    pub log: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTaskWithDate {
    pub status: TaskStatus,
    pub text: String,
    pub line: usize,
    pub log: String,
    pub date: String,
    #[serde(default)]
    pub context: Vec<String>,
}

// === Tree types ===

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileInput {
    pub file_name: String,
    pub file_uri: String,
    pub lines: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TreeTaskData {
    pub status: TaskStatus,
    pub text: String,
    /// 判断メタデータを取り除いたタスク本文（[`pj::split_decision_meta`]）
    pub body: String,
    /// 行末の判断メタデータ（`30分・軽・@PC`）。無ければ `None`
    pub meta: Option<String>,
    pub file_uri: String,
    pub line: usize,
    pub log: String,
    pub date: String,
    #[serde(default)]
    pub context: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TreeFileGroup {
    pub file_name: String,
    pub file_uri: String,
    pub tasks: Vec<TreeTaskData>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TreeDateGroup {
    pub date_key: String,
    pub label: String,
    pub is_today: bool,
    pub file_groups: Vec<TreeFileGroup>,
    pub completed_count: usize,
    pub total_count: usize,
}

// === Schedule types ===

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEntry {
    pub task_text: String,
    pub task_line: usize,
    pub status: TaskStatus,
    pub log_text: String,
    pub log_line: usize,
    pub time: String,
    pub end_time: String,
    pub file_uri: String,
}

// === Parsing logic ===

pub fn parse_tasks_internal(lines: &[String], target_date: &str) -> Vec<ParsedTask> {
    let task_re = Regex::new(r"^([ \t]*)-[ \t]*\[([ x-])\][ \t]*(.*)").unwrap();
    let date_re = Regex::new(r"^([ \t]*)-[ \t]*(\d{4}-\d{2}-\d{2})(?:[ \t]+\d{1,2}:\d{2}(?:-\d{1,2}:\d{2})?)?:[ \t]*(.*)").unwrap();
    let fence_re = Regex::new(r"^[ \t]*(```|~~~)").unwrap();

    let mut tasks: Vec<ParsedTask> = Vec::new();
    let mut current_task: Option<CurrentTask> = None;
    let mut in_code_block = false;

    for (i, text) in lines.iter().enumerate() {
        if fence_re.is_match(text) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        if let Some(caps) = task_re.captures(text) {
            current_task = Some(CurrentTask {
                indent: caps[1].len(),
                status: TaskStatus::from_marker(&caps[2]),
                text: caps[3].to_string(),
                line: i,
                context: vec![],
            });
            continue;
        }

        if let Some(caps) = date_re.captures(text) {
            if let Some(ref ct) = current_task {
                let date_indent = caps[1].len();
                let date_str = &caps[2];
                let log_content = &caps[3];

                if date_str == target_date && date_indent > ct.indent {
                    tasks.push(ParsedTask {
                        status: ct.status,
                        text: ct.text.clone(),
                        line: ct.line,
                        log: log_content.to_string(),
                    });
                }
            }
        }
    }

    tasks
}

pub fn parse_all_dates_internal(lines: &[String]) -> Vec<ParsedTaskWithDate> {
    let task_re = Regex::new(r"^([ \t]*)-[ \t]*\[([ x-])\][ \t]*(.*)").unwrap();
    let date_re = Regex::new(r"^([ \t]*)-[ \t]*(\d{4}-\d{2}-\d{2})(?:[ \t]+\d{1,2}:\d{2}(?:-\d{1,2}:\d{2})?)?:[ \t]*(.*)").unwrap();
    let heading_re = Regex::new(r"^(#{1,6})[ \t]+(.*)").unwrap();
    let fence_re = Regex::new(r"^[ \t]*(```|~~~)").unwrap();

    let mut tasks: Vec<ParsedTaskWithDate> = Vec::new();
    let mut current_task: Option<CurrentTask> = None;
    let mut current_task_has_log = false;
    let mut current_headings: Vec<String> = Vec::new();
    let mut in_code_block = false;

    for (i, text) in lines.iter().enumerate() {
        if fence_re.is_match(text) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        if let Some(caps) = heading_re.captures(text) {
            let level = caps[1].len();
            current_headings.truncate(level - 1);
            current_headings.push(caps[2].to_string());
            continue;
        }

        if let Some(caps) = task_re.captures(text) {
            if let Some(ref ct) = current_task {
                if !current_task_has_log {
                    tasks.push(ParsedTaskWithDate {
                        status: ct.status,
                        text: ct.text.clone(),
                        line: ct.line,
                        log: String::new(),
                        date: String::new(),
                        context: ct.context.clone(),
                    });
                }
            }
            current_task = Some(CurrentTask {
                indent: caps[1].len(),
                status: TaskStatus::from_marker(&caps[2]),
                text: caps[3].to_string(),
                line: i,
                context: current_headings.clone(),
            });
            current_task_has_log = false;
            continue;
        }

        if let Some(caps) = date_re.captures(text) {
            if let Some(ref ct) = current_task {
                let date_indent = caps[1].len();
                let date_str = &caps[2];
                let log_content = &caps[3];

                if date_indent > ct.indent {
                    tasks.push(ParsedTaskWithDate {
                        status: ct.status,
                        text: ct.text.clone(),
                        line: ct.line,
                        log: log_content.to_string(),
                        date: date_str.to_string(),
                        context: ct.context.clone(),
                    });
                    current_task_has_log = true;
                }
            }
        }
    }

    if let Some(ref ct) = current_task {
        if !current_task_has_log {
            tasks.push(ParsedTaskWithDate {
                status: ct.status,
                text: ct.text.clone(),
                line: ct.line,
                log: String::new(),
                date: String::new(),
                context: ct.context.clone(),
            });
        }
    }

    tasks
}

pub fn build_tree_data_internal(files: Vec<FileInput>, today_str: &str) -> Vec<TreeDateGroup> {
    // date -> Vec<(file_name, file_uri, tasks)>
    let mut date_map: HashMap<String, Vec<(String, String, Vec<TreeTaskData>)>> = HashMap::new();

    for file in &files {
        let parsed = parse_all_dates_internal(&file.lines);
        if parsed.is_empty() {
            continue;
        }

        // Group by date within this file
        let mut by_date: HashMap<String, Vec<TreeTaskData>> = HashMap::new();
        for t in parsed {
            // 判断メタデータの分離は PJ ノートの `## 次の予定` と同じルールを使う。
            // 利用側（skill 等）で切り出し直させると判定が二重実装になるため。
            let (body, meta) = match pj::split_decision_meta(&t.text) {
                Some((body, meta)) => (body, Some(meta)),
                None => (t.text.clone(), None),
            };
            by_date
                .entry(t.date.clone())
                .or_default()
                .push(TreeTaskData {
                    status: t.status,
                    text: t.text,
                    body,
                    meta,
                    file_uri: file.file_uri.clone(),
                    line: t.line,
                    log: t.log,
                    date: t.date,
                    context: t.context,
                });
        }

        for (date, tasks) in by_date {
            date_map.entry(date).or_default().push((
                file.file_name.clone(),
                file.file_uri.clone(),
                tasks,
            ));
        }
    }

    let mut result: Vec<TreeDateGroup> = Vec::new();

    // 今日のタスク（全タスク表示 + 進捗計算）
    if let Some(groups) = date_map.remove(today_str) {
        let mut file_groups: Vec<TreeFileGroup> = Vec::new();
        let mut completed_count = 0;
        let mut total_count = 0;

        for (file_name, file_uri, mut tasks) in groups {
            // 見送り(Cancelled)はカウント母数に含めない（中立）
            total_count += tasks
                .iter()
                .filter(|t| t.status != TaskStatus::Cancelled)
                .count();
            completed_count += tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Completed)
                .count();
            // 未完了→完了→見送り の順にソート
            tasks.sort_by_key(|t| match t.status {
                TaskStatus::Incomplete => 0,
                TaskStatus::Completed => 1,
                TaskStatus::Cancelled => 2,
            });
            file_groups.push(TreeFileGroup {
                file_name,
                file_uri,
                tasks,
            });
        }

        if !file_groups.is_empty() {
            result.push(TreeDateGroup {
                label: format!(
                    "今日 ({}) ({}/{})",
                    today_str, completed_count, total_count
                ),
                date_key: today_str.to_string(),
                is_today: true,
                file_groups,
                completed_count,
                total_count,
            });
        }
    }

    // その他の日付（降順、未完了のみ、全完了なら除外）
    let mut other_dates: Vec<String> = date_map.keys().filter(|d| !d.is_empty()).cloned().collect();
    other_dates.sort();
    other_dates.reverse();

    for date in other_dates {
        let groups = date_map.remove(&date).unwrap();
        let mut file_groups: Vec<TreeFileGroup> = Vec::new();
        let mut has_incomplete = false;

        for (file_name, file_uri, tasks) in groups {
            let incomplete_tasks: Vec<TreeTaskData> = tasks
                .into_iter()
                .filter(|t| t.status == TaskStatus::Incomplete)
                .collect();
            if !incomplete_tasks.is_empty() {
                has_incomplete = true;
                file_groups.push(TreeFileGroup {
                    file_name,
                    file_uri,
                    tasks: incomplete_tasks,
                });
            }
        }

        if has_incomplete {
            result.push(TreeDateGroup {
                label: date.clone(),
                date_key: date,
                is_today: false,
                file_groups,
                completed_count: 0,
                total_count: 0,
            });
        }
    }

    // 日付なし（未完了のみ）
    if let Some(groups) = date_map.remove("") {
        let mut file_groups: Vec<TreeFileGroup> = Vec::new();
        let mut has_incomplete = false;

        for (file_name, file_uri, tasks) in groups {
            let incomplete_tasks: Vec<TreeTaskData> = tasks
                .into_iter()
                .filter(|t| t.status == TaskStatus::Incomplete)
                .collect();
            if !incomplete_tasks.is_empty() {
                has_incomplete = true;
                file_groups.push(TreeFileGroup {
                    file_name,
                    file_uri,
                    tasks: incomplete_tasks,
                });
            }
        }

        if has_incomplete {
            result.push(TreeDateGroup {
                label: "日付なし".to_string(),
                date_key: String::new(),
                is_today: false,
                file_groups,
                completed_count: 0,
                total_count: 0,
            });
        }
    }

    result
}

// === Schedule parsing ===

fn pad_time(time: &str) -> String {
    if time.is_empty() {
        return String::new();
    }
    // "9:30" → "09:30"
    if time.len() == 4 {
        format!("0{}", time)
    } else {
        time.to_string()
    }
}

pub fn parse_schedule_internal(lines: &[String], target_date: &str) -> Vec<ScheduleEntry> {
    let task_re = Regex::new(r"^([ \t]*)-[ \t]*\[([ x-])\][ \t]*(.*)").unwrap();
    let date_re =
        Regex::new(r"^([ \t]*)-[ \t]*(\d{4}-\d{2}-\d{2})(?:[ \t]+(\d{1,2}:\d{2})(?:-(\d{1,2}:\d{2}))?)?:[ \t]*(.*)").unwrap();
    let time_memo_re = Regex::new(r"^-[ \t](\d{1,2}:\d{2}):[ \t](.+)").unwrap();
    let heading_date_re = Regex::new(r"^#[ \t]+(\d{4}-\d{2}-\d{2})").unwrap();
    let fence_re = Regex::new(r"^[ \t]*(```|~~~)").unwrap();

    // ジャーナルファイルの日付見出しが target_date と一致するか判定。
    // フェンス内は全解析から除外するので、ここでも走査しながら内外を追う
    // （記法の例として ``` の中に書いた `# YYYY-MM-DD` で時刻メモを発火させない）
    let is_target_date_file = {
        let mut in_code_block = false;
        lines.iter().any(|line| {
            if fence_re.is_match(line) {
                in_code_block = !in_code_block;
                return false;
            }
            if in_code_block {
                return false;
            }
            heading_date_re
                .captures(line)
                .is_some_and(|caps| &caps[1] == target_date)
        })
    };

    let mut entries: Vec<ScheduleEntry> = Vec::new();
    let mut current_task: Option<CurrentTask> = None;
    let mut in_code_block = false;

    for (i, text) in lines.iter().enumerate() {
        if fence_re.is_match(text) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        if let Some(caps) = task_re.captures(text) {
            current_task = Some(CurrentTask {
                indent: caps[1].len(),
                status: TaskStatus::from_marker(&caps[2]),
                text: caps[3].to_string(),
                line: i,
                context: vec![],
            });
            continue;
        }

        if let Some(caps) = date_re.captures(text) {
            if let Some(ref ct) = current_task {
                let date_indent = caps[1].len();
                let date_str = &caps[2];
                let time_str = caps.get(3).map_or("", |m| m.as_str());
                let end_time_str = caps.get(4).map_or("", |m| m.as_str());
                let log_content = &caps[5];

                if date_str == target_date && date_indent > ct.indent {
                    // 時刻を2桁にパディング
                    let time_padded = pad_time(time_str);
                    let end_time_padded = pad_time(end_time_str);
                    entries.push(ScheduleEntry {
                        task_text: ct.text.clone(),
                        task_line: ct.line,
                        status: ct.status,
                        log_text: log_content.to_string(),
                        log_line: i,
                        time: time_padded,
                        end_time: end_time_padded,
                        file_uri: String::new(),
                    });
                }
            }
            continue;
        }

        // 時刻メモ: ジャーナルファイル内のトップレベル「- HH:MM: テキスト」行
        if is_target_date_file {
            if let Some(caps) = time_memo_re.captures(text) {
                let time_str = &caps[1];
                let memo_text = &caps[2];
                let time_padded = pad_time(time_str);
                entries.push(ScheduleEntry {
                    task_text: String::new(),
                    task_line: i,
                    status: TaskStatus::Incomplete,
                    log_text: memo_text.to_string(),
                    log_line: i,
                    time: time_padded,
                    end_time: String::new(),
                    file_uri: String::new(),
                });
            }
        }
    }

    entries
}

pub fn build_schedule_data_internal(
    files: Vec<FileInput>,
    target_date: &str,
) -> Vec<ScheduleEntry> {
    let mut all_entries: Vec<ScheduleEntry> = Vec::new();

    for file in &files {
        let mut entries = parse_schedule_internal(&file.lines, target_date);
        for entry in &mut entries {
            entry.file_uri = file.file_uri.clone();
        }
        all_entries.extend(entries);
    }

    // 時刻順にソート（空文字は末尾）
    all_entries.sort_by(|a, b| {
        match (a.time.is_empty(), b.time.is_empty()) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => a.time.cmp(&b.time),
        }
    });

    all_entries
}

// === Front matter ===

/// front matter の `project:` に書ける 3 つの値。
///
/// 完了日を含まない「種別」なので、`--status` の絞り込みと表示ラベルはこちらで行う。
/// 関与の状態そのものは [`ProjectState`]。
#[derive(serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Active,
    Someday,
    Done,
}

/// PJ の関与の状態（docs/domain.md §2）。
///
/// `completed` は `done` のときだけ意味を持つので状態の中に畳む。独立したフィールドに
/// すると `active` かつ `completed: 2026-01-01` という無意味な組み合わせが表現できて
/// しまう。front matter の 2 キー表記（`project:` / `completed:`）は変えず、読み取った
/// あとの型だけを畳んでいる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectState {
    Active,
    Someday,
    Done { completed: Option<String> },
}

impl ProjectState {
    /// 完了日を落とした種別。
    pub fn status(&self) -> ProjectStatus {
        match self {
            ProjectState::Active => ProjectStatus::Active,
            ProjectState::Someday => ProjectStatus::Someday,
            ProjectState::Done { .. } => ProjectStatus::Done,
        }
    }

    /// 完了日。`done` 以外では常に `None`。
    pub fn completed(&self) -> Option<&str> {
        match self {
            ProjectState::Done { completed } => completed.as_deref(),
            _ => None,
        }
    }
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct FrontMatter {
    #[serde(default)]
    pub project: Option<ProjectStatus>,
    /// 作業実体が別リポジトリにある PJ のみ指定する（例: `~/workspace/dicegame`）
    #[serde(default)]
    pub repo: Option<String>,
    /// `project: done` のときの完了日（`YYYY-MM-DD`）
    #[serde(default)]
    pub completed: Option<String>,
}

pub fn parse_front_matter(lines: &[String]) -> Option<FrontMatterParsed> {
    if lines.is_empty() || lines[0].trim_end() != "---" {
        return None;
    }
    let mut end: Option<usize> = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim_end() == "---" {
            end = Some(i);
            break;
        }
    }
    let end = end?;
    let body = lines[1..end].join("\n");
    if body.trim().is_empty() {
        return Some(FrontMatterParsed::default());
    }
    let fm: FrontMatter = serde_yml::from_str(&body).ok()?;
    Some(FrontMatterParsed {
        state: fm.project.map(|status| match status {
            ProjectStatus::Active => ProjectState::Active,
            ProjectStatus::Someday => ProjectState::Someday,
            // `completed:` を拾うのは `done` のときだけ。`active` に添えられた完了日は
            // 意味を持たないので、ここで落として型から表現できなくする
            ProjectStatus::Done => ProjectState::Done {
                completed: fm.completed,
            },
        }),
        repo: fm.repo,
    })
}

#[derive(Debug, Default, PartialEq)]
pub struct FrontMatterParsed {
    /// `project:` が無ければ `None`（＝ PJ ノートではない）
    pub state: Option<ProjectState>,
    pub repo: Option<String>,
}

// === Tag extraction ===

/// 行単位で何度も呼ばれるのでコンパイル結果を使い回す（`journal_work` は
/// journal のタスク行ごとにこれを引く）。
fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#([^\s#]+)").unwrap())
}

pub fn extract_tags(text: &str) -> Vec<String> {
    tag_re()
        .captures_iter(text)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// `project: active` が front matter にあればファイル名 (拡張子除去・空白は `_`) を
/// 唯一のタグとして返す。`someday`（棚上げ）・`done`・未指定の場合は空配列。
pub fn extract_file_tags(lines: &[String], file_name: &str) -> Vec<String> {
    let Some(fm) = parse_front_matter(lines) else {
        return Vec::new();
    };
    if fm.state.as_ref().map(ProjectState::status) != Some(ProjectStatus::Active) {
        return Vec::new();
    }
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
    let tag = stem.replace(' ', "_");
    if tag.is_empty() {
        return Vec::new();
    }
    vec![tag]
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn s(str: &str) -> String {
        str.to_string()
    }

    fn lines(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    // --- parse_tasks_internal tests ---

    #[test]
    fn test_parse_tasks_basic_incomplete() {
        let l = lines(&["- [ ] タスクA", "    - 2026-02-01: ログA"]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, TaskStatus::Incomplete);
        assert_eq!(result[0].text, "タスクA");
        assert_eq!(result[0].log, "ログA");
        assert_eq!(result[0].line, 0);
    }

    #[test]
    fn test_parse_tasks_basic_completed() {
        let l = lines(&["- [x] 完了タスク", "    - 2026-02-01: 完了ログ"]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, TaskStatus::Completed);
        assert_eq!(result[0].text, "完了タスク");
        assert_eq!(result[0].log, "完了ログ");
    }

    #[test]
    fn test_parse_tasks_only_target_date() {
        let l = lines(&[
            "- [ ] タスク",
            "    - 2026-01-31: 昨日のログ",
            "    - 2026-02-01: 今日のログ",
            "    - 2026-02-02: 明日のログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].log, "今日のログ");
    }

    #[test]
    fn test_parse_tasks_multiple() {
        let l = lines(&[
            "- [ ] タスク1",
            "    - 2026-02-01: ログ1",
            "- [x] タスク2",
            "    - 2026-02-01: ログ2",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "タスク1");
        assert_eq!(result[1].text, "タスク2");
        assert_eq!(result[1].status, TaskStatus::Completed);
    }

    #[test]
    fn test_parse_tasks_same_date_multiple_logs() {
        let l = lines(&[
            "- [ ] タスク",
            "    - 2026-02-01: ログA",
            "    - 2026-02-01: ログB",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].log, "ログA");
        assert_eq!(result[1].log, "ログB");
    }

    #[test]
    fn test_parse_tasks_shallow_indent_ignored() {
        let l = lines(&[
            "    - [ ] タスク（インデント4）",
            "    - 2026-02-01: 同レベルのログ",
            "  - 2026-02-01: 浅いインデントのログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_tasks_no_task_with_log() {
        let l = lines(&["    - 2026-02-01: 孤立したログ"]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_tasks_empty() {
        let result = parse_tasks_internal(&[], "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_tasks_no_log() {
        let l = lines(&["- [ ] ログなしタスク"]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_tasks_line_numbers() {
        let l = lines(&[
            "# ヘッダー",
            "",
            "- [ ] 3行目のタスク",
            "    - 2026-02-01: ログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 2);
    }

    #[test]
    fn test_parse_tasks_with_unrelated_lines() {
        let l = lines(&[
            "- [ ] タスク1",
            "    - 2026-02-01: ログ1",
            "",
            "これは普通のテキスト",
            "",
            "- [x] タスク2",
            "    - 2026-02-01: ログ2",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "タスク1");
        assert_eq!(result[1].text, "タスク2");
    }

    #[test]
    fn test_parse_tasks_in_code_block_ignored() {
        let l = lines(&[
            "- [ ] 通常タスク",
            "    - 2026-02-01: ログ",
            "```",
            "- [ ] コードブロック内タスク",
            "    - 2026-02-01: コード内ログ",
            "```",
            "- [ ] コードブロック後タスク",
            "    - 2026-02-01: 後ログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "通常タスク");
        assert_eq!(result[1].text, "コードブロック後タスク");
    }

    #[test]
    fn test_parse_tasks_in_tilde_fence_ignored() {
        let l = lines(&[
            "~~~",
            "- [ ] チルダフェンス内",
            "    - 2026-02-01: ログ",
            "~~~",
            "- [ ] フェンス外",
            "    - 2026-02-01: 外ログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "フェンス外");
    }

    #[test]
    fn test_parse_tasks_in_code_block_with_lang() {
        let l = lines(&[
            "```markdown",
            "- [ ] 例として書かれたタスク",
            "    - 2026-02-01: 例ログ",
            "```",
            "- [ ] 実タスク",
            "    - 2026-02-01: 実ログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "実タスク");
    }

    #[test]
    fn test_parse_tasks_nested() {
        let l = lines(&[
            "- [ ] 親タスク",
            "    - [ ] 子タスク",
            "        - 2026-02-01: 子のログ",
        ]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "子タスク");
        assert_eq!(result[0].log, "子のログ");
    }

    // --- parse_all_dates_internal tests ---

    #[test]
    fn test_parse_all_dates_basic() {
        let l = lines(&[
            "- [ ] タスクA",
            "    - 2026-01-31: 昨日のログ",
            "    - 2026-02-01: 今日のログ",
            "    - 2026-02-02: 明日のログ",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].date, "2026-01-31");
        assert_eq!(result[0].log, "昨日のログ");
        assert_eq!(result[1].date, "2026-02-01");
        assert_eq!(result[1].log, "今日のログ");
        assert_eq!(result[2].date, "2026-02-02");
        assert_eq!(result[2].log, "明日のログ");
    }

    #[test]
    fn test_parse_all_dates_multiple_tasks() {
        let l = lines(&[
            "- [ ] タスク1",
            "    - 2026-02-01: ログ1",
            "- [x] タスク2",
            "    - 2026-01-30: ログ2",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "タスク1");
        assert_eq!(result[0].date, "2026-02-01");
        assert_eq!(result[1].text, "タスク2");
        assert_eq!(result[1].date, "2026-01-30");
    }

    #[test]
    fn test_parse_all_dates_shallow_indent_ignored() {
        let l = lines(&[
            "    - [ ] タスク（インデント4）",
            "    - 2026-02-01: 同レベルのログ",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "タスク（インデント4）");
        assert_eq!(result[0].date, "");
        assert_eq!(result[0].log, "");
    }

    #[test]
    fn test_parse_all_dates_empty_input() {
        let result = parse_all_dates_internal(&[]);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_all_dates_no_log_task() {
        let l = lines(&["- [ ] ログなしタスク"]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "ログなしタスク");
        assert_eq!(result[0].date, "");
        assert_eq!(result[0].log, "");
    }

    #[test]
    fn test_parse_all_dates_mixed() {
        let l = lines(&[
            "- [ ] タスク1",
            "    - 2026-02-01: ログ1",
            "- [ ] タスク2",
            "- [x] タスク3",
            "    - 2026-01-30: ログ3",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].text, "タスク1");
        assert_eq!(result[0].date, "2026-02-01");
        assert_eq!(result[1].text, "タスク2");
        assert_eq!(result[1].date, "");
        assert_eq!(result[2].text, "タスク3");
        assert_eq!(result[2].date, "2026-01-30");
    }

    #[test]
    fn test_parse_all_dates_in_code_block_ignored() {
        let l = lines(&[
            "- [ ] 実タスク",
            "    - 2026-02-01: ログ",
            "```",
            "- [ ] コード内タスク",
            "    - 2026-02-01: コード内ログ",
            "```",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "実タスク");
    }

    #[test]
    fn test_parse_all_dates_heading_in_code_block_ignored() {
        // コードブロック内の見出しは context として扱わない
        let l = lines(&[
            "# 本物の見出し",
            "```",
            "## コード内見出し",
            "```",
            "- [ ] タスク",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].context, vec!["本物の見出し"]);
    }

    // --- 記法上の空白は半角スペースとタブだけ（docs/syntax.md §3） ---

    #[test]
    fn test_full_width_space_indent_is_not_a_task() {
        // `\s` は全角スペースにも一致するので、放っておくと `　- [ ] 本文` が
        // タスクになる。Markdown のリストでもないものを拾わない
        let l = lines(&["　- [ ] 全角スペース字下げ"]);
        assert_eq!(parse_all_dates_internal(&l).len(), 0);
    }

    #[test]
    fn test_full_width_space_between_marker_and_checkbox_is_not_a_task() {
        let l = lines(&["-　[ ] 全角スペース区切り"]);
        assert_eq!(parse_all_dates_internal(&l).len(), 0);
    }

    #[test]
    fn test_full_width_space_indented_log_does_not_attach() {
        // タスクは字下げ無し、ログは全角スペース字下げ。ログ行としても一致しないので、
        // タスクは「ログを持たない」= 日付なし 1 件になる
        let l = lines(&["- [ ] タスク", "　- 2026-02-01: ログ"]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].date, "");
        assert_eq!(result[0].log, "");
    }

    #[test]
    fn test_tab_indent_attaches_log() {
        let l = lines(&["- [ ] タスク", "\t- 2026-02-01: ログ"]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].date, "2026-02-01");
        assert_eq!(result[0].log, "ログ");
    }

    #[test]
    fn test_full_width_space_heading_is_not_a_heading() {
        let l = lines(&["#　全角スペース見出し", "- [ ] タスク"]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert!(result[0].context.is_empty());
    }

    // --- parse_all_dates_internal context tests ---

    #[test]
    fn test_parse_all_dates_heading_context() {
        let l = lines(&[
            "# プロジェクトA",
            "## 仕事",
            "- [ ] タスク1",
            "    - 2026-02-01: ログ1",
            "## 個人",
            "- [ ] タスク2",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result[0].context, vec!["プロジェクトA", "仕事"]);
        assert_eq!(result[1].context, vec!["プロジェクトA", "個人"]);
    }

    #[test]
    fn test_parse_all_dates_no_heading_empty_context() {
        let l = lines(&["- [ ] タスク"]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result[0].context, Vec::<String>::new());
    }

    #[test]
    fn test_parse_all_dates_heading_level_change() {
        let l = lines(&[
            "# H1",
            "## H2",
            "### H3",
            "- [ ] deep task",
            "## H2b",
            "- [ ] shallower task",
        ]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result[0].context, vec!["H1", "H2", "H3"]);
        assert_eq!(result[1].context, vec!["H1", "H2b"]);
    }

    // --- build_tree_data_internal tests ---

    #[test]
    fn test_build_tree_data_empty() {
        let result = build_tree_data_internal(vec![], "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_tree_data_today_all_tasks_shown() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [ ] 未完了タスク",
                "    - 2026-02-01: ログ1",
                "- [x] 完了タスク",
                "    - 2026-02-01: ログ2",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert!(result[0].is_today);
        assert_eq!(result[0].completed_count, 1);
        assert_eq!(result[0].total_count, 2);
        assert_eq!(result[0].label, "今日 (2026-02-01) (1/2)");
        let tasks = &result[0].file_groups[0].tasks;
        assert_eq!(tasks.len(), 2);
        // 未完了が先
        assert_eq!(tasks[0].status, TaskStatus::Incomplete);
        assert_eq!(tasks[1].status, TaskStatus::Completed);
    }

    #[test]
    fn test_build_tree_data_splits_decision_meta() {
        // `taski pj` と同じルールで判断メタデータを分離する。
        // 利用側で切り出し直させると同じ判定の二重実装になるため。
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [ ] 資料をまとめる（30分・軽・@PC）",
                "    - 2026-02-01: ログ1",
                "- [ ] テクスチャペイント（髪・服・顔）",
                "    - 2026-02-01: ログ2",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        let tasks = &result[0].file_groups[0].tasks;
        assert_eq!(tasks.len(), 2);

        assert_eq!(tasks[0].text, "資料をまとめる（30分・軽・@PC）");
        assert_eq!(tasks[0].body, "資料をまとめる");
        assert_eq!(tasks[0].meta.as_deref(), Some("30分・軽・@PC"));

        // 列挙はメタデータではない。body は原文のまま
        assert_eq!(tasks[1].body, "テクスチャペイント（髪・服・顔）");
        assert_eq!(tasks[1].meta, None);
    }

    #[test]
    fn test_parse_tasks_cancelled() {
        let l = lines(&["- [-] 見送りタスク", "    - 2026-02-01: ログ"]);
        let result = parse_tasks_internal(&l, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, TaskStatus::Cancelled);
        assert_eq!(result[0].text, "見送りタスク");
    }

    #[test]
    fn test_build_tree_data_today_cancelled_shown_not_counted() {
        // 見送り(Cancelled)は今日ビューに表示されるが、カウント母数(分母・分子)に含めない
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [ ] 未完了タスク",
                "    - 2026-02-01: ログ1",
                "- [x] 完了タスク",
                "    - 2026-02-01: ログ2",
                "- [-] 見送りタスク",
                "    - 2026-02-01: ログ3",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 1);
        // 見送りは母数に含めないので 1/2 のまま
        assert_eq!(result[0].completed_count, 1);
        assert_eq!(result[0].total_count, 2);
        assert_eq!(result[0].label, "今日 (2026-02-01) (1/2)");
        let tasks = &result[0].file_groups[0].tasks;
        // 表示はされる（3件）。順序は 未完了→完了→見送り
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].status, TaskStatus::Incomplete);
        assert_eq!(tasks[1].status, TaskStatus::Completed);
        assert_eq!(tasks[2].status, TaskStatus::Cancelled);
    }

    #[test]
    fn test_build_tree_data_other_date_cancelled_hidden() {
        // 他日付ビューでは見送りは未完了として扱わず非表示（完了と同じ扱い）
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [-] 見送りタスク",
                "    - 2026-01-30: ログ1",
                "- [ ] 未完了タスク",
                "    - 2026-01-30: ログ2",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 1);
        let tasks = &result[0].file_groups[0].tasks;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, TaskStatus::Incomplete);
    }

    #[test]
    fn test_build_tree_data_other_date_all_cancelled_excluded() {
        // 他日付で見送りのみなら、その日付グループごと除外される
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&["- [-] 見送りタスク", "    - 2026-01-30: ログ"]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_tree_data_other_date_incomplete_only() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [ ] 未完了タスク",
                "    - 2026-01-30: ログ1",
                "- [x] 完了タスク",
                "    - 2026-01-30: ログ2",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert!(!result[0].is_today);
        assert_eq!(result[0].date_key, "2026-01-30");
        let tasks = &result[0].file_groups[0].tasks;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, TaskStatus::Incomplete);
    }

    #[test]
    fn test_build_tree_data_other_date_all_complete_excluded() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&["- [x] 完了タスク", "    - 2026-01-30: ログ"]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_tree_data_no_date() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&["- [ ] 日付なしタスク"]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].date_key, "");
        assert_eq!(result[0].label, "日付なし");
    }

    #[test]
    fn test_build_tree_data_no_date_all_complete_excluded() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&["- [x] 完了日付なしタスク"]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_tree_data_sort_order() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [ ] 今日のタスク",
                "    - 2026-02-01: ログ",
                "- [ ] 古いタスク",
                "    - 2026-01-15: ログ",
                "- [ ] 昨日のタスク",
                "    - 2026-01-31: ログ",
                "- [ ] 日付なしタスク",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 4);
        // 今日が最初
        assert!(result[0].is_today);
        assert_eq!(result[0].date_key, "2026-02-01");
        // 降順
        assert_eq!(result[1].date_key, "2026-01-31");
        assert_eq!(result[2].date_key, "2026-01-15");
        // 日付なしが最後
        assert_eq!(result[3].date_key, "");
    }

    #[test]
    fn test_build_tree_data_multiple_files() {
        let files = vec![
            FileInput {
                file_name: s("file1.md"),
                file_uri: s("file:///file1.md"),
                lines: lines(&["- [ ] タスク1", "    - 2026-02-01: ログ1"]),
            },
            FileInput {
                file_name: s("file2.md"),
                file_uri: s("file:///file2.md"),
                lines: lines(&["- [ ] タスク2", "    - 2026-02-01: ログ2"]),
            },
        ];
        let result = build_tree_data_internal(files, "2026-02-01");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_groups.len(), 2);
    }

    #[test]
    fn test_build_tree_data_task_sort_incomplete_first() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [x] 完了タスク1",
                "    - 2026-02-01: ログ1",
                "- [ ] 未完了タスク",
                "    - 2026-02-01: ログ2",
                "- [x] 完了タスク2",
                "    - 2026-02-01: ログ3",
            ]),
        }];
        let result = build_tree_data_internal(files, "2026-02-01");
        let tasks = &result[0].file_groups[0].tasks;
        assert_eq!(tasks[0].status, TaskStatus::Incomplete);
        assert_eq!(tasks[1].status, TaskStatus::Completed);
        assert_eq!(tasks[2].status, TaskStatus::Completed);
    }

    // --- parse_schedule_internal tests ---

    #[test]
    fn test_parse_schedule_with_time() {
        let l = lines(&["- [ ] タスクA", "    - 2026-03-21 09:00: ミーティング"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task_text, "タスクA");
        assert_eq!(result[0].time, "09:00");
        assert_eq!(result[0].log_text, "ミーティング");
        assert_eq!(result[0].status, TaskStatus::Incomplete);
    }

    #[test]
    fn test_parse_schedule_no_time() {
        let l = lines(&["- [ ] タスクA", "    - 2026-03-21: ログ"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, "");
        assert_eq!(result[0].log_text, "ログ");
    }

    #[test]
    fn test_parse_schedule_log_line() {
        let l = lines(&[
            "# ヘッダー",
            "",
            "- [ ] タスク",
            "    - 2026-03-21 10:00: ログ",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task_line, 2);
        assert_eq!(result[0].log_line, 3);
    }

    #[test]
    fn test_parse_schedule_indent_rules() {
        let l = lines(&[
            "    - [ ] タスク（インデント4）",
            "    - 2026-03-21 09:00: 同レベルのログ",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_schedule_multiple_times() {
        let l = lines(&[
            "- [ ] タスクA",
            "    - 2026-03-21 09:00: 朝",
            "    - 2026-03-21 14:00: 午後",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].time, "09:00");
        assert_eq!(result[0].log_text, "朝");
        assert_eq!(result[1].time, "14:00");
        assert_eq!(result[1].log_text, "午後");
    }

    #[test]
    fn test_parse_schedule_wrong_date_ignored() {
        let l = lines(&[
            "- [ ] タスク",
            "    - 2026-03-20 09:00: 昨日のログ",
            "    - 2026-03-21 09:00: 今日のログ",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].log_text, "今日のログ");
    }

    #[test]
    fn test_parse_schedule_completed_task() {
        let l = lines(&["- [x] 完了タスク", "    - 2026-03-21 10:00: 完了"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, TaskStatus::Completed);
    }

    #[test]
    fn test_parse_schedule_cancelled_task() {
        let l = lines(&["- [-] 見送りタスク", "    - 2026-03-21 10:00: 見送り"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, TaskStatus::Cancelled);
    }

    #[test]
    fn test_parse_schedule_in_code_block_ignored() {
        let l = lines(&[
            "- [ ] 実タスク",
            "    - 2026-03-21 09:00: 朝",
            "```",
            "- [ ] コード内タスク",
            "    - 2026-03-21 10:00: コード内ログ",
            "```",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task_text, "実タスク");
    }

    #[test]
    fn test_parse_schedule_time_memo_in_code_block_ignored() {
        let l = lines(&[
            "# 2026-03-21",
            "",
            "- 09:00: 通常メモ",
            "```",
            "- 10:00: コード内メモ",
            "```",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].log_text, "通常メモ");
    }

    // --- parse_schedule_internal time memo tests ---

    #[test]
    fn test_parse_schedule_time_memo_in_journal() {
        let l = lines(&[
            "# 2026-03-21",
            "",
            "- 09:30: 散歩した",
            "- 14:00: コーヒー飲んだ",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].time, "09:30");
        assert_eq!(result[0].log_text, "散歩した");
        assert_eq!(result[0].task_text, "");
        assert_eq!(result[0].status, TaskStatus::Incomplete);
        assert_eq!(result[1].time, "14:00");
        assert_eq!(result[1].log_text, "コーヒー飲んだ");
    }

    #[test]
    fn test_parse_schedule_time_memo_single_digit_hour() {
        let l = lines(&["# 2026-03-21", "", "- 9:30: 朝の散歩"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, "09:30");
        assert_eq!(result[0].log_text, "朝の散歩");
    }

    #[test]
    fn test_parse_schedule_time_memo_wrong_date_ignored() {
        let l = lines(&["# 2026-03-20", "", "- 09:30: 昨日のメモ"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_schedule_time_memo_no_heading_ignored() {
        // 日付見出しがないファイルでは時刻メモを拾わない
        let l = lines(&["- 09:30: メモ"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_schedule_time_memo_heading_in_code_block_ignored() {
        // フェンス内の日付見出しは見出しではないので、時刻メモを発火させない。
        // 記法の例をコードブロックに書いただけのファイルが journal 扱いになるのを防ぐ
        let l = lines(&["```markdown", "# 2026-03-21", "```", "", "- 09:30: メモ"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_schedule_time_memo_heading_after_code_block_fires() {
        // フェンスを閉じた後の日付見出しは通常どおり効く
        let l = lines(&[
            "```markdown",
            "# 2026-03-20",
            "```",
            "# 2026-03-21",
            "",
            "- 09:30: メモ",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].log_text, "メモ");
    }

    #[test]
    fn test_parse_schedule_time_memo_accepts_tab_separator() {
        // 記法上の空白は半角スペースとタブ（docs/syntax.md §3）
        let l = lines(&["# 2026-03-21", "", "-\t09:30:\tタブ区切りのメモ"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, "09:30");
        assert_eq!(result[0].log_text, "タブ区切りのメモ");
    }

    #[test]
    fn test_parse_schedule_time_memo_rejects_full_width_space() {
        // 全角スペースは記法上の空白ではない
        let l = lines(&["# 2026-03-21", "", "-　09:30:　全角区切り"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_schedule_time_memo_mixed_with_tasks() {
        let l = lines(&[
            "# 2026-03-21",
            "",
            "- 09:00: 朝ごはん",
            "- [ ] タスクA",
            "    - 2026-03-21 10:00: 作業開始",
            "- 12:00: 昼休み",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 3);
        // 時刻メモ
        assert_eq!(result[0].time, "09:00");
        assert_eq!(result[0].log_text, "朝ごはん");
        assert_eq!(result[0].task_text, "");
        // タスクログ
        assert_eq!(result[1].time, "10:00");
        assert_eq!(result[1].log_text, "作業開始");
        assert_eq!(result[1].task_text, "タスクA");
        // 時刻メモ
        assert_eq!(result[2].time, "12:00");
        assert_eq!(result[2].log_text, "昼休み");
        assert_eq!(result[2].task_text, "");
    }

    #[test]
    fn test_parse_schedule_indented_time_not_memo() {
        // インデントされた行は時刻メモとして拾わない
        let l = lines(&["# 2026-03-21", "", "  - 09:30: インデントあり"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 0);
    }

    // --- build_schedule_data_internal tests ---

    #[test]
    fn test_build_schedule_data_sorted_by_time() {
        let files = vec![FileInput {
            file_name: s("test.md"),
            file_uri: s("file:///test.md"),
            lines: lines(&[
                "- [ ] タスクB",
                "    - 2026-03-21 14:00: 午後",
                "- [ ] タスクA",
                "    - 2026-03-21 09:00: 朝",
                "- [ ] タスクC",
                "    - 2026-03-21: 時刻なし",
            ]),
        }];
        let result = build_schedule_data_internal(files, "2026-03-21");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].time, "09:00");
        assert_eq!(result[0].task_text, "タスクA");
        assert_eq!(result[1].time, "14:00");
        assert_eq!(result[1].task_text, "タスクB");
        assert_eq!(result[2].time, "");
        assert_eq!(result[2].task_text, "タスクC");
    }

    #[test]
    fn test_build_schedule_data_multiple_files() {
        let files = vec![
            FileInput {
                file_name: s("file1.md"),
                file_uri: s("file:///file1.md"),
                lines: lines(&["- [ ] タスク1", "    - 2026-03-21 09:00: ログ1"]),
            },
            FileInput {
                file_name: s("file2.md"),
                file_uri: s("file:///file2.md"),
                lines: lines(&["- [ ] タスク2", "    - 2026-03-21 10:00: ログ2"]),
            },
        ];
        let result = build_schedule_data_internal(files, "2026-03-21");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file_uri, "file:///file1.md");
        assert_eq!(result[1].file_uri, "file:///file2.md");
    }

    #[test]
    fn test_parse_schedule_time_range() {
        let l = lines(&[
            "- [ ] ミーティング",
            "    - 2026-03-21 13:00-14:00: 定例会議",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, "13:00");
        assert_eq!(result[0].end_time, "14:00");
        assert_eq!(result[0].task_text, "ミーティング");
        assert_eq!(result[0].log_text, "定例会議");
    }

    #[test]
    fn test_parse_schedule_time_range_single_digit() {
        let l = lines(&[
            "- [ ] 朝会",
            "    - 2026-03-21 9:00-9:30: スタンドアップ",
        ]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, "09:00");
        assert_eq!(result[0].end_time, "09:30");
    }

    #[test]
    fn test_parse_schedule_no_end_time() {
        let l = lines(&["- [ ] タスク", "    - 2026-03-21 10:00: ログ"]);
        let result = parse_schedule_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, "10:00");
        assert_eq!(result[0].end_time, "");
    }

    #[test]
    fn test_parse_tasks_with_time() {
        // 時刻付きログもタスクとして認識される
        let l = lines(&["- [ ] タスク", "    - 2026-03-21 09:00: ログ"]);
        let result = parse_tasks_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "タスク");
        assert_eq!(result[0].log, "ログ");
    }

    #[test]
    fn test_parse_tasks_with_time_range() {
        // 時間範囲付きログもタスクとして認識される
        let l = lines(&["- [ ] 会議", "    - 2026-03-21 13:00-14:00: 定例"]);
        let result = parse_tasks_internal(&l, "2026-03-21");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "会議");
        assert_eq!(result[0].log, "定例");
    }

    #[test]
    fn test_parse_all_dates_with_time() {
        let l = lines(&["- [ ] タスク", "    - 2026-03-21 10:00: ログ"]);
        let result = parse_all_dates_internal(&l);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].date, "2026-03-21");
        assert_eq!(result[0].log, "ログ");
    }

    // --- parse_front_matter tests ---

    #[test]
    fn test_parse_front_matter_project_active() {
        let l = lines(&[
            "---",
            "project: active",
            "---",
            "",
            "- [ ] タスク",
        ]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.state, Some(ProjectState::Active));
    }

    #[test]
    fn test_parse_front_matter_project_someday() {
        let l = lines(&["---", "project: someday", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.state, Some(ProjectState::Someday));
    }

    #[test]
    fn test_parse_front_matter_project_done() {
        let l = lines(&["---", "project: done", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.state, Some(ProjectState::Done { completed: None }));
    }

    #[test]
    fn test_parse_front_matter_project_missing() {
        let l = lines(&["---", "other: value", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.state, None);
    }

    #[test]
    fn test_parse_front_matter_project_invalid_value_is_none() {
        let l = lines(&["---", "project: foo", "---"]);
        assert!(parse_front_matter(&l).is_none());
    }

    #[test]
    fn test_parse_front_matter_none_when_no_leading_delimiter() {
        let l = lines(&["", "---", "project: active", "---"]);
        assert!(parse_front_matter(&l).is_none());
    }

    #[test]
    fn test_parse_front_matter_none_when_unclosed() {
        let l = lines(&["---", "project: active", "- [ ] タスク"]);
        assert!(parse_front_matter(&l).is_none());
    }

    #[test]
    fn test_parse_front_matter_empty_body() {
        let l = lines(&["---", "---"]);
        let fm = parse_front_matter(&l).expect("empty front matter is valid");
        assert_eq!(fm.state, None);
    }

    #[test]
    fn test_parse_front_matter_invalid_yaml() {
        let l = lines(&["---", "project: [unclosed", "---"]);
        assert!(parse_front_matter(&l).is_none());
    }

    #[test]
    fn test_parse_front_matter_repo() {
        let l = lines(&["---", "project: active", "repo: ~/workspace/dicegame", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.repo.as_deref(), Some("~/workspace/dicegame"));
    }

    #[test]
    fn test_parse_front_matter_completed_unquoted_date() {
        // クォートなしの日付が String として読めること（実データはこの書き方）
        let l = lines(&["---", "project: done", "completed: 2026-05-31", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(
            fm.state,
            Some(ProjectState::Done {
                completed: Some(s("2026-05-31")),
            })
        );
    }

    #[test]
    fn test_parse_front_matter_completed_is_dropped_unless_done() {
        // `active` に添えられた完了日は状態として無意味なので型から落とす（G-9）
        let l = lines(&["---", "project: active", "completed: 2026-05-31", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.state, Some(ProjectState::Active));
        assert_eq!(fm.state.as_ref().and_then(ProjectState::completed), None);
    }

    #[test]
    fn test_parse_front_matter_repo_absent() {
        let l = lines(&["---", "project: active", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.repo, None);
        assert_eq!(fm.state.as_ref().and_then(ProjectState::completed), None);
    }

    #[test]
    fn test_parse_front_matter_unknown_field_ignored() {
        let l = lines(&["---", "project: active", "future_field: 何か", "---"]);
        let fm = parse_front_matter(&l).expect("front matter should parse");
        assert_eq!(fm.state, Some(ProjectState::Active));
    }

    // --- extract_file_tags tests ---

    #[test]
    fn test_extract_file_tags_project_active_uses_filename_stem() {
        let l = lines(&["---", "project: active", "---", "- [ ] タスク"]);
        assert_eq!(extract_file_tags(&l, "projectA.md"), vec![s("projectA")]);
    }

    #[test]
    fn test_extract_file_tags_replaces_spaces_with_underscore() {
        let l = lines(&["---", "project: active", "---"]);
        assert_eq!(
            extract_file_tags(&l, "2026-04-14 会議メモ.md"),
            vec![s("2026-04-14_会議メモ")]
        );
    }

    #[test]
    fn test_extract_file_tags_project_someday() {
        let l = lines(&["---", "project: someday", "---"]);
        let empty: Vec<String> = vec![];
        assert_eq!(extract_file_tags(&l, "projectA.md"), empty);
    }

    #[test]
    fn test_extract_file_tags_project_done() {
        let l = lines(&["---", "project: done", "---"]);
        let empty: Vec<String> = vec![];
        assert_eq!(extract_file_tags(&l, "projectA.md"), empty);
    }

    #[test]
    fn test_extract_file_tags_no_front_matter() {
        let l = lines(&["- [ ] タスク"]);
        let empty: Vec<String> = vec![];
        assert_eq!(extract_file_tags(&l, "projectA.md"), empty);
    }

    #[test]
    fn test_extract_file_tags_ignores_tags_field() {
        // 旧仕様の tags: は無視される
        let l = lines(&["---", "tags:", "  - old", "---"]);
        let empty: Vec<String> = vec![];
        assert_eq!(extract_file_tags(&l, "projectA.md"), empty);
    }

    #[test]
    fn test_extract_file_tags_filename_without_md_extension() {
        let l = lines(&["---", "project: active", "---"]);
        assert_eq!(extract_file_tags(&l, "plain"), vec![s("plain")]);
    }

    #[test]
    fn test_extract_file_tags_mid_file_delimiter_ignored() {
        let l = lines(&[
            "- [ ] タスク",
            "---",
            "project: active",
            "---",
        ]);
        let empty: Vec<String> = vec![];
        assert_eq!(extract_file_tags(&l, "projectA.md"), empty);
    }
}
