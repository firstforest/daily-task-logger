//! `taski pj` — PJ 横断の状態を集約するサブコマンド。
//!
//! `taski list` が日付軸なのに対し、こちらは PJ 軸。
//! `note/*.md` のうち front matter に `project:` を持つノートを対象に、
//! 「次の予定」「ログの鮮度」「リポジトリの未反映コミット」を集める。
//!
//! **判断はしない。** 機械的に決まる事実だけを出し、
//! 「このタスクは粗い」「これは AI に投げるべき」といった判断は skill 側に任せる。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use chrono::{Local, NaiveDate};
use parser_core::pj::{parse_pj_note, PjHealth, PjLogEntry};
use parser_core::{parse_front_matter, ProjectStatus};
use regex::Regex;
use serde::Serialize;
use unicode_width::UnicodeWidthStr;

/// JSON に出す1 PJ 分の集約結果。
#[derive(Serialize, Debug, Clone)]
pub struct PjProject {
    pub name: String,
    pub path: String,
    pub status: String,
    pub repo: Option<String>,
    pub completed: Option<String>,
    pub next_action: Option<String>,
    pub next_action_body: Option<String>,
    pub next_action_meta: Option<String>,
    pub next_action_ai: bool,
    pub health: PjHealth,
    /// PJ ノート自体の最終更新日（git 基準）
    pub updated: Option<String>,
    pub stale_days: Option<i64>,
    pub log_last: Option<String>,
    pub log_days: Option<i64>,
    pub repo_last: Option<String>,
    pub repo_days: Option<i64>,
    /// リポジトリにコミットがあるのに PJ ノートのログに反映されていないか
    pub unreported: bool,
    pub unreported_count: usize,
    pub journal_last: Option<String>,
    pub journal_days: Option<i64>,
    pub backlog_count: usize,
    pub backlog: Vec<String>,
    /// 再開時のコンテキストとして使う直近のログ
    pub logs: Vec<PjLogEntry>,
}

#[derive(Serialize, Debug)]
pub struct PjOutput {
    pub generated: String,
    pub projects: Vec<PjProject>,
}

/// JSON に返すログの件数。再開時のコンテキストとして使うので直近数件で足りる。
const LOG_LIMIT: usize = 3;

/// table 表示で「次の予定」の本文に割り当てる表示幅。超えたら切り詰める。
const NEXT_ACTION_WIDTH: usize = 36;

fn status_label(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Active => "active",
        ProjectStatus::Someday => "someday",
        ProjectStatus::Done => "done",
    }
}

fn parse_status_filter(spec: &str) -> Result<Vec<ProjectStatus>, String> {
    let mut result = Vec::new();
    for raw in spec.split(',') {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        let status = match s {
            "active" => ProjectStatus::Active,
            "someday" => ProjectStatus::Someday,
            "done" => ProjectStatus::Done,
            other => return Err(other.to_string()),
        };
        if !result.contains(&status) {
            result.push(status);
        }
    }
    Ok(result)
}

/// 2つの `YYYY-MM-DD` の差を日数で返す。
fn days_between(from: &str, to: &str) -> Option<i64> {
    let from = NaiveDate::parse_from_str(from, "%Y-%m-%d").ok()?;
    let to = NaiveDate::parse_from_str(to, "%Y-%m-%d").ok()?;
    Some((to - from).num_days())
}

/// `~/...` をホームディレクトリに展開する。
fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// `git log --name-only --format=%x00%ad` の出力から、パス → 最終更新日を作る。
///
/// git log は新しい順に出るので、最初に現れたものが最終更新日になる。
fn parse_git_name_only(output: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut current_date: Option<String> = None;

    for line in output.lines() {
        if let Some(date) = line.strip_prefix('\0') {
            current_date = Some(date.trim().to_string());
            continue;
        }
        let path = line.trim();
        if path.is_empty() {
            continue;
        }
        if let Some(ref date) = current_date {
            result.entry(path.to_string()).or_insert_with(|| date.clone());
        }
    }

    result
}

/// note/ 配下の各ファイルの最終更新日を git から1回でまとめて取る。
///
/// `-c core.quotepath=false` が無いと日本語ファイル名が8進エスケープされて一致しない。
fn note_last_updated(base_dir: &Path) -> HashMap<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(base_dir)
        .args([
            "-c",
            "core.quotepath=false",
            "log",
            "--name-only",
            "--format=%x00%ad",
            "--date=short",
            "--",
            "note/",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            parse_git_name_only(&String::from_utf8_lossy(&out.stdout))
        }
        _ => HashMap::new(),
    }
}

/// journal ファイルを新しい順に並べる（ファイル名が `YYYY-MM-DD.md` のものだけを対象にする）。
fn journal_files_desc(base_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_journal_files(&base_dir.join("journal"), &mut files);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files
}

fn collect_journal_files(dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_journal_files(&path, files);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        if NaiveDate::parse_from_str(&stem, "%Y-%m-%d").is_ok() {
            files.push((stem, path));
        }
    }
}

/// journal で各 PJ が最後に言及された日を取る。
///
/// 新しい日付から順に見て、全 PJ が見つかった時点で打ち切る。
/// 空白を含む PJ 名は `#タグ` で書けないので、実質 `[[名前]]` のみが効く。
fn journal_last_mentions(base_dir: &Path, names: &[String]) -> HashMap<String, String> {
    let mut result: HashMap<String, String> = HashMap::new();
    if names.is_empty() {
        return result;
    }

    let tag_re = Regex::new(r"#([^\s#]+)").unwrap();
    // PJ 名 → (`[[名前]]` の検索文字列, ファイル単位タグ)
    let targets: Vec<(String, String, String)> = names
        .iter()
        .map(|name| (name.clone(), format!("[[{name}]]"), name.replace(' ', "_")))
        .collect();

    let mut remaining: HashSet<&str> = targets.iter().map(|(n, _, _)| n.as_str()).collect();

    for (date, path) in journal_files_desc(base_dir) {
        if remaining.is_empty() {
            break;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let tags: HashSet<&str> = tag_re
            .captures_iter(&content)
            .filter_map(|caps| caps.get(1).map(|m| m.as_str()))
            .collect();

        for (name, link, tag) in &targets {
            if !remaining.contains(name.as_str()) {
                continue;
            }
            if content.contains(link) || tags.contains(tag.as_str()) {
                result.insert(name.clone(), date.clone());
                remaining.remove(name.as_str());
            }
        }
    }

    result
}

/// 未反映かどうか。`repo_last > log_last` のときだけ true。
///
/// 判定が厳密大なりである以上、`log_last` 当日のコミットは「反映済み」と見なす。
/// 件数を数えるクエリ側も当日を除外しないとフラグと食い違う（`repo_info` を参照）。
/// ログが1件も無い PJ は、コミットがある時点で未反映とする。
fn is_unreported(repo_last: Option<&str>, log_last: Option<&str>) -> bool {
    match (repo_last, log_last) {
        (Some(repo_last), Some(log_last)) => repo_last > log_last,
        (Some(_), None) => true,
        _ => false,
    }
}

struct RepoInfo {
    last: Option<String>,
    unreported_count: usize,
}

/// `repo:` のリポジトリから最終コミット日と未反映コミット数を取る。
///
/// - `git log -1`（HEAD 基準）だけでは未マージの作業ブランチを取りこぼすので `--branches --remotes` を使う
/// - `--all` は使わない。jj (Jujutsu) の `refs/jj/keep/*` を拾って件数が数倍に膨らむ
/// - リポジトリが存在しない / git でない場合は `None` を返して続行する
fn repo_info(repo: &str, log_last: Option<&str>) -> RepoInfo {
    let path = expand_home(repo);
    if !path.exists() {
        return RepoInfo {
            last: None,
            unreported_count: 0,
        };
    }

    let last = git_output(&path, &["log", "-1", "--format=%ad", "--date=short", "--branches", "--remotes"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if last.is_none() {
        return RepoInfo {
            last: None,
            unreported_count: 0,
        };
    }

    // `--since=<log_last>` は log_last 当日 00:00 以降を含むので使わない。
    // unreported の判定が厳密大なりである以上、当日のコミットは反映済みと見なす必要がある。
    let mut args: Vec<String> = vec![
        "log".into(),
        "--format=%H".into(),
        "--branches".into(),
        "--remotes".into(),
    ];
    if let Some(log_last) = log_last {
        args.push(format!("--after={log_last} 23:59:59"));
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let unreported_count = git_output(&path, &arg_refs)
        .map(|out| out.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);

    RepoInfo {
        last,
        unreported_count,
    }
}

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn collect_note_files(note_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let Ok(entries) = fs::read_dir(note_dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// 手を入れる必要が高い順に並べる。
///
/// 1. 未反映（`!`）を最優先。作業は進んでいるのに taski が捉えられていない状態
/// 2. ログが古い / 無い順
/// 3. health（次の予定なし → 未 clarify → ok）
/// 4. PJ 名
fn sort_key(p: &PjProject) -> (u8, i64, u8, String) {
    let unreported_rank = if p.unreported { 0 } else { 1 };
    // ログが無い PJ を最も古い扱いにする
    let log_rank = -p.log_days.unwrap_or(i64::MAX / 2);
    let health_rank = match p.health {
        PjHealth::NoNext => 0,
        PjHealth::Unclarified => 1,
        PjHealth::Ok => 2,
    };
    (unreported_rank, log_rank, health_rank, p.name.clone())
}

/// PJ を集める。git・journal の走査を含む。
fn collect_projects(base_dir: &Path, statuses: &[ProjectStatus], today: &str) -> Vec<PjProject> {
    let note_dir = base_dir.join("note");
    let updated_map = note_last_updated(base_dir);

    struct Pending {
        name: String,
        rel_path: String,
        status: ProjectStatus,
        repo: Option<String>,
        completed: Option<String>,
        note: parser_core::pj::PjNote,
    }

    let mut pending: Vec<Pending> = Vec::new();

    for path in collect_note_files(&note_dir) {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let Some(fm) = parse_front_matter(&lines) else {
            continue;
        };
        let Some(status) = fm.project else {
            continue;
        };
        if !statuses.contains(&status) {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let rel_path = path
            .strip_prefix(base_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        pending.push(Pending {
            name,
            rel_path,
            status,
            repo: fm.repo,
            completed: fm.completed,
            note: parse_pj_note(&lines),
        });
    }

    let names: Vec<String> = pending.iter().map(|p| p.name.clone()).collect();
    let mentions = journal_last_mentions(base_dir, &names);

    let mut projects: Vec<PjProject> = pending
        .into_iter()
        .map(|p| {
            let log_last = p.note.log_last().map(|s| s.to_string());
            let repo_data = p
                .repo
                .as_deref()
                .map(|repo| repo_info(repo, log_last.as_deref()));

            let repo_last = repo_data.as_ref().and_then(|r| r.last.clone());
            let unreported = is_unreported(repo_last.as_deref(), log_last.as_deref());
            let unreported_count = if unreported {
                repo_data.as_ref().map(|r| r.unreported_count).unwrap_or(0)
            } else {
                0
            };

            let updated = updated_map.get(&p.rel_path).cloned();
            let journal_last = mentions.get(&p.name).cloned();

            let mut logs = p.note.logs.clone();
            logs.truncate(LOG_LIMIT);

            PjProject {
                name: p.name,
                path: p.rel_path,
                status: status_label(p.status).to_string(),
                repo: p.repo,
                completed: p.completed,
                next_action: p.note.next_action,
                next_action_body: p.note.next_action_body,
                next_action_meta: p.note.next_action_meta,
                next_action_ai: p.note.next_action_ai,
                health: p.note.health,
                stale_days: updated.as_deref().and_then(|d| days_between(d, today)),
                updated,
                log_days: log_last.as_deref().and_then(|d| days_between(d, today)),
                log_last,
                repo_days: repo_last.as_deref().and_then(|d| days_between(d, today)),
                repo_last,
                unreported,
                unreported_count,
                journal_days: journal_last.as_deref().and_then(|d| days_between(d, today)),
                journal_last,
                backlog_count: p.note.backlog.len(),
                backlog: p.note.backlog,
                logs,
            }
        })
        .collect();

    projects.sort_by_key(sort_key);
    projects
}

// === 表示 ===

/// 東アジア文字幅を考慮して右側を空白で埋める。文字数で揃えると日本語で崩れる。
fn pad_display(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        return s.to_string();
    }
    format!("{}{}", s, " ".repeat(width - w))
}

/// 東アジア文字幅を考慮して左側を空白で埋める（番号など右揃えにしたい列用）。
fn pad_display_right(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        return s.to_string();
    }
    format!("{}{}", " ".repeat(width - w), s)
}

/// 表示幅で切り詰める。切り詰めたら末尾に `…` を付ける。
fn truncate_display(s: &str, width: usize) -> String {
    if UnicodeWidthStr::width(s) <= width {
        return s.to_string();
    }
    let mut result = String::new();
    let mut acc = 0;
    for c in s.chars() {
        let cw = UnicodeWidthStr::width(c.to_string().as_str());
        if acc + cw > width.saturating_sub(1) {
            break;
        }
        result.push(c);
        acc += cw;
    }
    result.push('…');
    result
}

fn days_cell(days: Option<i64>) -> String {
    match days {
        Some(d) => format!("{d}d"),
        None => "-".to_string(),
    }
}

fn health_label(health: PjHealth) -> &'static str {
    match health {
        PjHealth::Ok => "ok",
        PjHealth::Unclarified => "未clarify",
        PjHealth::NoNext => "NA不在",
    }
}

fn print_table(projects: &[PjProject], today: &str) {
    println!(
        "\x1b[1mPJ状態  {}  ({}件)\x1b[0m",
        today,
        projects.len()
    );
    println!();

    let idx_w = projects.len().to_string().len().max(1);
    let name_w = projects
        .iter()
        .map(|p| UnicodeWidthStr::width(p.name.as_str()) + 1) // `!` の分
        .chain(std::iter::once(4))
        .max()
        .unwrap_or(4);
    let days_w = 5;
    let health_w = projects
        .iter()
        .map(|p| UnicodeWidthStr::width(health_label(p.health)))
        .chain(std::iter::once(4))
        .max()
        .unwrap_or(9);

    let header = format!(
        "  {}  {}  {}  {}  {}  {}  次の予定",
        pad_display("#", idx_w),
        pad_display("PJ", name_w),
        pad_display("ログ", days_w),
        pad_display("repo", days_w),
        pad_display("言及", days_w),
        pad_display("状態", health_w),
    );

    // 罫線の長さを合わせるため、装飾なしの行を先に組んでから最大幅を測る
    let rows: Vec<(String, String)> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let marker = if p.unreported { "!" } else { " " };
            let name_cell = pad_display(&format!("{marker}{}", p.name), name_w);
            let name_colored = if p.unreported {
                format!("\x1b[33m{name_cell}\x1b[0m")
            } else {
                name_cell.clone()
            };
            let (next_plain, next_colored) = match (&p.next_action_body, &p.next_action_meta) {
                (Some(body), Some(meta)) => {
                    let body = truncate_display(body, NEXT_ACTION_WIDTH);
                    (
                        format!("{body}  {meta}"),
                        format!("{body}  \x1b[2m{meta}\x1b[0m"),
                    )
                }
                (Some(body), None) => {
                    let body = truncate_display(body, NEXT_ACTION_WIDTH);
                    (body.clone(), body)
                }
                (None, _) => (
                    "（次の予定なし）".to_string(),
                    "\x1b[2m（次の予定なし）\x1b[0m".to_string(),
                ),
            };
            let prefix = format!(
                "  {}  {}  {}  {}  {}  {}  ",
                pad_display_right(&(i + 1).to_string(), idx_w),
                name_cell,
                pad_display(&days_cell(p.log_days), days_w),
                pad_display(&days_cell(p.repo_days), days_w),
                pad_display(&days_cell(p.journal_days), days_w),
                pad_display(health_label(p.health), health_w),
            );
            let prefix_colored = format!(
                "  {}  {}  {}  {}  {}  {}  ",
                pad_display_right(&(i + 1).to_string(), idx_w),
                name_colored,
                pad_display(&days_cell(p.log_days), days_w),
                pad_display(&days_cell(p.repo_days), days_w),
                pad_display(&days_cell(p.journal_days), days_w),
                pad_display(health_label(p.health), health_w),
            );
            (
                format!("{prefix}{next_plain}"),
                format!("{prefix_colored}{next_colored}"),
            )
        })
        .collect();

    let rule_w = rows
        .iter()
        .map(|(plain, _)| UnicodeWidthStr::width(plain.as_str()))
        .chain(std::iter::once(UnicodeWidthStr::width(header.as_str())))
        .max()
        .unwrap_or(0);

    println!("\x1b[2m{header}\x1b[0m");
    println!("\x1b[2m  {}\x1b[0m", "─".repeat(rule_w.saturating_sub(2)));

    for (_, colored) in &rows {
        println!("{colored}");
    }

    let unclarified = projects
        .iter()
        .filter(|p| p.health == PjHealth::Unclarified)
        .count();
    let no_next = projects
        .iter()
        .filter(|p| p.health == PjHealth::NoNext)
        .count();
    let unreported = projects.iter().filter(|p| p.unreported).count();

    println!();
    println!("要clarify {unclarified}件 / NA不在 {no_next}件 / 未反映 {unreported}件");
    println!(
        "\x1b[2m  ログ = PJノートの最終ログ / repo = リポジトリ最終コミット / 言及 = journal で触れられた日\x1b[0m"
    );
    println!(
        "\x1b[2m  ! = 未反映（repo にコミットがあるのに PJノートのログに無い）\x1b[0m"
    );
}

/// `taski pj` の本体。
pub fn run(
    base_dir: PathBuf,
    format: Option<String>,
    status: Option<String>,
    all: bool,
    today: Option<String>,
) {
    if !base_dir.exists() {
        eprintln!("エラー: {} が見つかりません", base_dir.display());
        process::exit(1);
    }

    let today = today.unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    if NaiveDate::parse_from_str(&today, "%Y-%m-%d").is_err() {
        eprintln!("エラー: 日付は YYYY-MM-DD で指定してください: {today}");
        process::exit(1);
    }

    let statuses = if all {
        vec![
            ProjectStatus::Active,
            ProjectStatus::Someday,
            ProjectStatus::Done,
        ]
    } else {
        match parse_status_filter(status.as_deref().unwrap_or("active")) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                eprintln!("エラー: --status が空です");
                process::exit(1);
            }
            Err(bad) => {
                eprintln!("エラー: 未対応の status です: {bad}（active / someday / done）");
                process::exit(1);
            }
        }
    };

    let projects = collect_projects(&base_dir, &statuses, &today);

    match format.as_deref() {
        Some("json") => {
            let output = PjOutput {
                generated: today,
                projects,
            };
            let json = serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
                eprintln!("エラー: JSON変換に失敗しました: {e}");
                process::exit(1);
            });
            println!("{json}");
        }
        Some("table") | None => {
            if projects.is_empty() {
                println!("該当する PJ がありません");
                return;
            }
            print_table(&projects, &today);
        }
        Some(other) => {
            eprintln!("エラー: 未対応のフォーマットです: {other}");
            process::exit(1);
        }
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PjProject {
        PjProject {
            name: name.to_string(),
            path: format!("note/{name}.md"),
            status: "active".to_string(),
            repo: None,
            completed: None,
            next_action: None,
            next_action_body: None,
            next_action_meta: None,
            next_action_ai: false,
            health: PjHealth::Ok,
            updated: None,
            stale_days: None,
            log_last: None,
            log_days: None,
            repo_last: None,
            repo_days: None,
            unreported: false,
            unreported_count: 0,
            journal_last: None,
            journal_days: None,
            backlog_count: 0,
            backlog: vec![],
            logs: vec![],
        }
    }

    // --- status filter ---

    #[test]
    fn test_parse_status_filter_default() {
        assert_eq!(
            parse_status_filter("active").unwrap(),
            [ProjectStatus::Active]
        );
    }

    #[test]
    fn test_parse_status_filter_multiple() {
        assert_eq!(
            parse_status_filter("active,someday").unwrap(),
            [ProjectStatus::Active, ProjectStatus::Someday]
        );
    }

    #[test]
    fn test_parse_status_filter_dedupes_and_trims() {
        assert_eq!(
            parse_status_filter(" active , active ").unwrap(),
            [ProjectStatus::Active]
        );
    }

    #[test]
    fn test_parse_status_filter_invalid() {
        assert_eq!(parse_status_filter("paused"), Err("paused".to_string()));
    }

    // --- date ---

    #[test]
    fn test_days_between() {
        assert_eq!(days_between("2026-07-30", "2026-08-02"), Some(3));
        assert_eq!(days_between("2026-08-02", "2026-08-02"), Some(0));
    }

    #[test]
    fn test_days_between_invalid() {
        assert_eq!(days_between("なんか", "2026-08-02"), None);
    }

    // --- git log parsing ---

    #[test]
    fn test_parse_git_name_only_keeps_newest() {
        let output = "\u{0}2026-08-01\n\nnote/A.md\nnote/B.md\n\u{0}2026-07-20\n\nnote/A.md\nnote/C.md\n";
        let map = parse_git_name_only(output);
        assert_eq!(map.get("note/A.md").map(|s| s.as_str()), Some("2026-08-01"));
        assert_eq!(map.get("note/B.md").map(|s| s.as_str()), Some("2026-08-01"));
        assert_eq!(map.get("note/C.md").map(|s| s.as_str()), Some("2026-07-20"));
    }

    #[test]
    fn test_parse_git_name_only_handles_japanese_paths() {
        let output = "\u{0}2026-08-01\n\nnote/漫画制作エディタ.md\n";
        let map = parse_git_name_only(output);
        assert_eq!(
            map.get("note/漫画制作エディタ.md").map(|s| s.as_str()),
            Some("2026-08-01")
        );
    }

    #[test]
    fn test_parse_git_name_only_empty() {
        assert!(parse_git_name_only("").is_empty());
    }

    // --- unreported ---

    #[test]
    fn test_unreported_when_repo_is_newer() {
        assert!(is_unreported(Some("2026-07-26"), Some("2026-07-20")));
    }

    #[test]
    fn test_not_unreported_on_same_day() {
        // 境界。log_last 当日のコミットは反映済みと見なす
        assert!(!is_unreported(Some("2026-07-20"), Some("2026-07-20")));
    }

    #[test]
    fn test_not_unreported_when_log_is_newer() {
        assert!(!is_unreported(Some("2026-07-20"), Some("2026-07-26")));
    }

    #[test]
    fn test_unreported_when_no_log() {
        assert!(is_unreported(Some("2026-07-20"), None));
    }

    #[test]
    fn test_not_unreported_without_repo() {
        assert!(!is_unreported(None, Some("2026-07-20")));
        assert!(!is_unreported(None, None));
    }

    // --- home expansion ---

    #[test]
    fn test_expand_home() {
        std::env::set_var("HOME", "/home/test");
        assert_eq!(
            expand_home("~/workspace/x"),
            PathBuf::from("/home/test/workspace/x")
        );
        assert_eq!(expand_home("/abs/path"), PathBuf::from("/abs/path"));
    }

    // --- sort ---

    #[test]
    fn test_sort_unreported_first() {
        let mut a = project("A");
        a.log_days = Some(0);
        let mut b = project("B");
        b.log_days = Some(100);
        let mut c = project("C");
        c.log_days = Some(1);
        c.unreported = true;

        let mut list = [a, b, c];
        list.sort_by_key(sort_key);
        assert_eq!(list[0].name, "C");
        assert_eq!(list[1].name, "B");
        assert_eq!(list[2].name, "A");
    }

    #[test]
    fn test_sort_missing_log_is_oldest() {
        let mut a = project("A");
        a.log_days = Some(300);
        let b = project("B"); // ログなし

        let mut list = [a, b];
        list.sort_by_key(sort_key);
        assert_eq!(list[0].name, "B");
    }

    #[test]
    fn test_sort_health_breaks_tie() {
        let mut a = project("A");
        a.log_days = Some(5);
        a.health = PjHealth::Ok;
        let mut b = project("B");
        b.log_days = Some(5);
        b.health = PjHealth::NoNext;

        let mut list = [a, b];
        list.sort_by_key(sort_key);
        assert_eq!(list[0].name, "B");
    }

    // --- display width ---

    #[test]
    fn test_pad_display_japanese() {
        // 「永夜」は表示幅4なので、幅6に揃えると空白は2つ
        assert_eq!(pad_display("永夜", 6), "永夜  ");
    }

    #[test]
    fn test_pad_display_no_padding_when_over() {
        assert_eq!(pad_display("永夜祭", 2), "永夜祭");
    }

    #[test]
    fn test_truncate_display_ascii() {
        assert_eq!(truncate_display("abcdef", 4), "abc…");
    }

    #[test]
    fn test_truncate_display_japanese() {
        // 幅5に収めるので全角2文字（幅4）+ …
        assert_eq!(truncate_display("あいうえお", 5), "あい…");
    }

    #[test]
    fn test_truncate_display_fits() {
        assert_eq!(truncate_display("あい", 4), "あい");
    }

    #[test]
    fn test_days_cell() {
        assert_eq!(days_cell(Some(12)), "12d");
        assert_eq!(days_cell(None), "-");
    }
}
