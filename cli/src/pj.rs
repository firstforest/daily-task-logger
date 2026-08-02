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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use chrono::{Local, NaiveDate};
use parser_core::pj::{journal_work, parse_pj_note, PjHealth, PjLogEntry};
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
    /// `repo:` を home 展開・正規化した絶対パス。存在しなければ `None`。
    /// 利用側（セッションの `cwd` との突き合わせ等）に展開ロジックを再実装させないために出す
    pub repo_abs: Option<String>,
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
    /// `repo:` のリポジトリに remote が設定されているか。リポジトリが無い / git でないなら `None`
    pub has_remote: Option<bool>,
    /// remote に push されていないコミット数。remote 無し（`has_remote` が false / null）なら `None`
    pub ahead_count: Option<usize>,
    /// journal で最後に「言及」された日（`[[名前]]` / `#タグ`）
    pub journal_last: Option<String>,
    pub journal_days: Option<i64>,
    /// journal で最後に「実働」があった日。言及とは分けて持つ（[`parser_core::pj::journal_work`]）
    pub journal_work_last: Option<String>,
    pub journal_work_days: Option<i64>,
    pub backlog_count: usize,
    pub backlog: Vec<String>,
    /// 再開時のコンテキストとして使う直近のログ
    pub logs: Vec<PjLogEntry>,
}

#[derive(Serialize, Debug)]
pub struct PjOutput {
    pub generated: String,
    /// `repo:` のリポジトリを fetch したか。false のとき `repo_last` は古い可能性がある
    pub fetched: bool,
    /// fetch に失敗したリポジトリ。ここに挙がった PJ の `repo_last` は信用できない
    pub fetch_failed: Vec<String>,
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

/// 基準日（`--today`）より後の日付か。
///
/// 基準日より後のログ・コミット・言及は「その時点ではまだ無い」ものとして捨てる。
/// 残すと経過日数が負になり、`--today` で過去を振り返ったときに
/// 当時存在しなかったものが `-7d` のように並んで表になる。
/// `YYYY-MM-DD` は辞書順と日付順が一致するので文字列のまま比べる。
fn is_future(date: &str, today: &str) -> bool {
    date > today
}

/// `~/...` をホームディレクトリに展開する。
fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    expand_home_with(path, home.as_deref())
}

/// `expand_home` の本体。HOME を引数で受け取るのでテストからプロセス環境を触らずに済む。
fn expand_home_with(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// `git log --name-only --format=%x00%ad` の出力から、パス → 最終更新日を作る。
///
/// git log は新しい順に出るので、最初に現れたものが最終更新日になる。
/// 基準日より後のコミットは読み飛ばすので、`--today` に過去日を渡すと
/// その時点での最終更新日になる。
fn parse_git_name_only(output: &str, today: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut current_date: Option<String> = None;

    for line in output.lines() {
        if let Some(date) = line.strip_prefix('\0') {
            let date = date.trim().to_string();
            current_date = if is_future(&date, today) {
                None
            } else {
                Some(date)
            };
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
/// `--relative` が無いとパスがリポジトリルート相対で出るため、taski ディレクトリが
/// リポジトリ直下でない場合（dotfiles リポジトリ配下など）に `note/*.md` と一致しなくなる。
fn note_last_updated(base_dir: &Path, today: &str) -> HashMap<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(base_dir)
        .args([
            "-c",
            "core.quotepath=false",
            "log",
            "--name-only",
            "--relative",
            "--format=%x00%ad",
            "--date=short",
            "--",
            "note/",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            parse_git_name_only(&String::from_utf8_lossy(&out.stdout), today)
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

/// journal から取れる PJ ごとの日付。
///
/// 「言及」と「実働」を分けて持つ。言及だけを見ると、`## 今日の候補` に載っただけ・
/// 他タスクの文中で触れられただけの PJ まで「動いている」ように見えてしまう。
/// 停滞と言及を分けているのと同じ理由で、言及と実働も分ける。
struct JournalDates {
    /// `[[名前]]` / `#タグ` で最後に言及された日
    mention: HashMap<String, String>,
    /// 実働（完了したタスク・時刻付きログ）が最後にあった日
    work: HashMap<String, String>,
}

/// journal で各 PJ が最後に言及された日と、最後に実働があった日を取る。
///
/// 新しい日付から順に見て、全 PJ の両方が見つかった時点で打ち切る。
/// 言及と実働で同じファイルを2度読まないよう、1回の走査でまとめて集める。
/// 基準日より後の journal（明日のタスクを先に書いた場合など）は見ない。
/// 空白を含む PJ 名は `#タグ` で書けないので、実質 `[[名前]]` のみが効く。
fn journal_dates(base_dir: &Path, names: &[String], today: &str) -> JournalDates {
    let mut mention: HashMap<String, String> = HashMap::new();
    let mut work: HashMap<String, String> = HashMap::new();
    if names.is_empty() {
        return JournalDates { mention, work };
    }

    let tag_re = Regex::new(r"#([^\s#]+)").unwrap();
    // PJ 名 → (`[[名前]]` の検索文字列, ファイル単位タグ)
    let targets: Vec<(String, String, String)> = names
        .iter()
        .map(|name| (name.clone(), format!("[[{name}]]"), name.replace(' ', "_")))
        .collect();

    let mut remaining_mention: HashSet<&str> = targets.iter().map(|(n, _, _)| n.as_str()).collect();
    let mut remaining_work: HashSet<&str> = remaining_mention.clone();

    for (date, path) in journal_files_desc(base_dir) {
        if remaining_mention.is_empty() && remaining_work.is_empty() {
            break;
        }
        if is_future(&date, today) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        if !remaining_mention.is_empty() {
            let tags: HashSet<&str> = tag_re
                .captures_iter(&content)
                .filter_map(|caps| caps.get(1).map(|m| m.as_str()))
                .collect();

            for (name, link, tag) in &targets {
                if !remaining_mention.contains(name.as_str()) {
                    continue;
                }
                if content.contains(link) || tags.contains(tag.as_str()) {
                    mention.insert(name.clone(), date.clone());
                    remaining_mention.remove(name.as_str());
                }
            }
        }

        if remaining_work.is_empty() {
            continue;
        }
        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        // 参照名 → そのファイルでの最新の実働日。時刻付きログは自分の日付を持つので、
        // ファイル名の日付と一致しないことがある
        let works = journal_work(&lines, &date);
        let mut latest: HashMap<&str, &str> = HashMap::new();
        for w in works.iter().filter(|w| !is_future(&w.date, today)) {
            for r in &w.refs {
                let entry = latest.entry(r.as_str()).or_insert(w.date.as_str());
                if w.date.as_str() > *entry {
                    *entry = w.date.as_str();
                }
            }
        }
        for (name, _, tag) in &targets {
            if !remaining_work.contains(name.as_str()) {
                continue;
            }
            let found = [latest.get(name.as_str()), latest.get(tag.as_str())]
                .into_iter()
                .flatten()
                .max();
            if let Some(found) = found {
                work.insert(name.clone(), found.to_string());
                remaining_work.remove(name.as_str());
            }
        }
    }

    JournalDates { mention, work }
}

/// 未反映かどうか。`repo_last > log_last` のときだけ true。
///
/// 判定が厳密大なりである以上、`log_last` 当日のコミットは「反映済み」と見なす。
/// 件数側も同じ比較で数えないとフラグと食い違う（`repo_info` を参照）。
/// ログが1件も無い PJ は、コミットがある時点で未反映とする。
fn is_unreported(repo_last: Option<&str>, log_last: Option<&str>) -> bool {
    match (repo_last, log_last) {
        (Some(repo_last), Some(log_last)) => repo_last > log_last,
        (Some(_), None) => true,
        _ => false,
    }
}

/// 同時に走らせる `git fetch` の本数。
const FETCH_PARALLELISM: usize = 8;

/// `repo:` のリポジトリを fetch する。失敗したリポジトリのパスを返す。
///
/// fetch しない限り「ローカルの clone が古い」ことは検出できない
/// （remote-tracking ref 自体が古いので比較対象にならない）ため、既定で叩く。
/// PJ 数だけネットワークを使うので並列に流し、失敗しても集計は続行する。
fn fetch_repos(repos: &[PathBuf]) -> Vec<PathBuf> {
    if repos.is_empty() {
        return Vec::new();
    }

    // 静的に分割すると遅い1リポジトリがその担当分を丸ごと止めるので、
    // 空いたスレッドが次を取りに行く形にする。
    let next = AtomicUsize::new(0);
    let failed = Mutex::new(Vec::new());
    let workers = FETCH_PARALLELISM.min(repos.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (next, failed) = (&next, &failed);
            scope.spawn(move || {
                let mut local = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(repo) = repos.get(i) else {
                        break;
                    };
                    if !fetch_repo(repo) {
                        local.push(repo.clone());
                    }
                }
                if !local.is_empty() {
                    failed
                        .lock()
                        .expect("fetch の結果を回収できません")
                        .extend(local);
                }
            });
        }
    });

    let mut failed = failed.into_inner().expect("fetch の結果を回収できません");
    failed.sort();
    failed
}

/// SSH 接続の待ち時間の上限（秒）。到達不能なホストで OS 既定の TCP タイムアウトまで
/// 固まらないようにする。
const SSH_CONNECT_TIMEOUT: u32 = 10;

/// 1リポジトリを fetch する。リモートを持たないローカル専用リポジトリは成功扱い。
///
/// 認証待ちで固まらないよう対話プロンプトを潰し、遅い接続で止まらないよう
/// HTTP の低速打ち切りと SSH の接続タイムアウトを入れる。
fn fetch_repo(repo: &Path) -> bool {
    let has_remote = git_output(repo, &["remote"])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false);
    if !has_remote {
        return true;
    }

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args(["fetch", "--quiet"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1000")
        .env("GIT_HTTP_LOW_SPEED_TIME", "20");
    // 利用者が独自の GIT_SSH_COMMAND を持っている場合は壊さない
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env(
            "GIT_SSH_COMMAND",
            format!("ssh -oBatchMode=yes -oConnectTimeout={SSH_CONNECT_TIMEOUT}"),
        );
    }

    cmd.output().map(|out| out.status.success()).unwrap_or(false)
}

struct RepoInfo {
    last: Option<String>,
    unreported_count: usize,
    /// remote が設定されているか。git リポジトリでなければ `None`
    has_remote: Option<bool>,
    /// remote に無いコミット数。remote 未設定なら `None`
    ahead_count: Option<usize>,
}

impl RepoInfo {
    fn empty() -> Self {
        RepoInfo {
            last: None,
            unreported_count: 0,
            has_remote: None,
            ahead_count: None,
        }
    }
}

/// `git log` からコミット日（`YYYY-MM-DD`）を取る。
///
/// 日付は `%ad`（author date）で揃える。`--after` 等で git に数えさせると
/// committer date をローカルタイムゾーンで解釈するため基準が食い違う（`repo_info` 参照）。
fn git_log_dates(path: &Path, revs: &[&str]) -> Vec<String> {
    let mut args = vec!["log", "--format=%ad", "--date=short"];
    args.extend_from_slice(revs);
    git_output(path, &args)
        .map(|out| {
            out.lines()
                .map(|l| l.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// コミット日（`YYYY-MM-DD`）を全件返す。
///
/// - `git log -1`（HEAD 基準）だけでは未マージの作業ブランチを取りこぼすので `--branches --remotes` を使う
/// - `--all` は使わない。jj (Jujutsu) の `refs/jj/keep/*` を拾って件数が数倍に膨らむ
fn commit_dates(path: &Path) -> Vec<String> {
    git_log_dates(path, &["--branches", "--remotes"])
}

/// remote に無いコミットの日付を返す（ローカルの全ブランチから辿れて remote から辿れないもの）。
///
/// ブランチごとの upstream 設定には依存させない。upstream 未設定の作業ブランチが
/// 数えられなくなるうえ、`--branches --remotes` で全ブランチを見る `commit_dates` と
/// 土俵が変わってしまうため。
fn unpushed_dates(path: &Path) -> Vec<String> {
    git_log_dates(path, &["--branches", "--not", "--remotes"])
}

/// `repo:` を突き合わせに使える絶対パスへ正規化する。
///
/// 利用側（セッションの `cwd` と PJ を前方一致で突き合わせる等）に `~` 展開を
/// 再実装させないために出す。プロセスの `cwd` はシンボリックリンクを解決した
/// 物理パスで報告されるので、こちらもそれに揃える。
/// ディレクトリが存在しなければ `None`。
fn repo_abs_path(repo: &str) -> Option<String> {
    let path = expand_home(repo);
    if !path.exists() {
        return None;
    }
    let resolved = fs::canonicalize(&path).unwrap_or(path);
    Some(resolved.to_string_lossy().to_string())
}

/// `repo:` のリポジトリから最終コミット日と未反映コミット数を取る。
///
/// 最終日も未反映件数も、1回のクエリで取った同じ日付の列から作る。
/// 件数だけ git 側（`--after`）に数えさせると日付の基準が食い違い、
/// フラグと件数が矛盾する: `%ad` は **author date** をコミット自身のタイムゾーンで
/// 描画するのに対し、`--after` は **committer date** を実行環境のローカル
/// タイムゾーンで解釈するため、同じデータでも `TZ` によって件数が変わってしまう。
///
/// 最終日に `git log -1` を使わないのも同じ理由で、`-1` は commit date 順の
/// 先頭1件から author date を出すので、rebase / cherry-pick で両者がずれると
/// 他のコミットより古い日付を返す。ここでは最大値を取る。
///
/// リポジトリが存在しない / git でない場合は `None` を返して続行する。
///
/// 基準日より後のコミットは数えない。`--today` に過去日を渡したときに
/// 当時まだ存在しないコミットで「未反映」と言わないため。
///
/// ここには既定の `today`（実行環境のローカル日付）とのタイムゾーン差という穴がある。
/// `%ad` は author date を **コミット自身のタイムゾーン** で描画するので、自分より東の
/// タイムゾーン（UTC+13 など）や時計のずれた環境で作られたコミットは、ローカルの「今日」より
/// 1日先の日付を持ちうる。それが除外されると `repo_last` が1つ古いコミットまで巻き戻り、
/// 未反映の作業があるのに `unreported` が false に倒れる。以前は `repo_days: -1` として
/// 見えていた分、誤りが可視から不可視に変わっている。個人の PJ ノート用途では
/// 発生頻度が低いので許容しているが、他所からのコミットが混ざるリポジトリを
/// 対象にするなら、基準日が既定のときだけ日数を 0 でクランプする等の対処が要る。
fn repo_info(repo: &str, log_last: Option<&str>, today: &str) -> RepoInfo {
    let path = expand_home(repo);
    if !path.exists() {
        return RepoInfo::empty();
    }

    // git リポジトリでなければ `git remote` 自体が失敗するので、その区別も兼ねる
    let has_remote = git_output(&path, &["remote"]).map(|out| !out.trim().is_empty());
    // remote が無ければ「push 済み / 未 push」という区別自体が無い
    let ahead_count = (has_remote == Some(true)).then(|| {
        unpushed_dates(&path)
            .into_iter()
            .filter(|d| !is_future(d, today))
            .count()
    });

    let dates: Vec<String> = commit_dates(&path)
        .into_iter()
        .filter(|d| !is_future(d, today))
        .collect();

    // `is_unreported` と同じ厳密大なりで数える。log_last 当日のコミットは反映済み。
    // ログが1件も無い PJ はコミット全件が未反映（`is_unreported` も true を返す）。
    let unreported_count = match log_last {
        Some(log_last) => dates.iter().filter(|d| d.as_str() > log_last).count(),
        None => dates.len(),
    };

    RepoInfo {
        last: dates.iter().max().cloned(),
        unreported_count,
        has_remote,
        ahead_count,
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
/// 集計結果と、その過程で fetch に失敗したリポジトリ。
struct Collected {
    projects: Vec<PjProject>,
    fetch_failed: Vec<String>,
}

fn collect_projects(
    base_dir: &Path,
    statuses: &[ProjectStatus],
    today: &str,
    fetch: bool,
) -> Collected {
    let note_dir = base_dir.join("note");
    let updated_map = note_last_updated(base_dir, today);

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
    let journal = journal_dates(base_dir, &names, today);

    // 1つのリポジトリを複数 PJ が共有することがあるので重複を潰してから fetch する
    let fetch_failed: Vec<String> = if fetch {
        let mut seen: Vec<PathBuf> = Vec::new();
        for p in &pending {
            if let Some(repo) = p.repo.as_deref() {
                let path = expand_home(repo);
                if path.exists() && !seen.contains(&path) {
                    seen.push(path);
                }
            }
        }
        fetch_repos(&seen)
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut projects: Vec<PjProject> = pending
        .into_iter()
        .map(|p| {
            // 基準日より後のログは「その時点ではまだ書かれていない」ものとして落とす。
            // logs は新しい順なので、残った先頭が基準日時点の最新ログになる。
            let mut logs: Vec<PjLogEntry> = p
                .note
                .logs
                .into_iter()
                .filter(|l| !is_future(&l.date, today))
                .collect();
            let log_last = logs.first().map(|l| l.date.clone());
            let repo_data = p
                .repo
                .as_deref()
                .map(|repo| repo_info(repo, log_last.as_deref(), today));

            let repo_last = repo_data.as_ref().and_then(|r| r.last.clone());
            let unreported = is_unreported(repo_last.as_deref(), log_last.as_deref());
            let unreported_count = if unreported {
                repo_data.as_ref().map(|r| r.unreported_count).unwrap_or(0)
            } else {
                0
            };

            let updated = updated_map.get(&p.rel_path).cloned();
            let journal_last = journal.mention.get(&p.name).cloned();
            let journal_work_last = journal.work.get(&p.name).cloned();

            logs.truncate(LOG_LIMIT);

            PjProject {
                name: p.name,
                path: p.rel_path,
                status: status_label(p.status).to_string(),
                repo_abs: p.repo.as_deref().and_then(repo_abs_path),
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
                has_remote: repo_data.as_ref().and_then(|r| r.has_remote),
                ahead_count: repo_data.as_ref().and_then(|r| r.ahead_count),
                journal_days: journal_last.as_deref().and_then(|d| days_between(d, today)),
                journal_last,
                journal_work_days: journal_work_last
                    .as_deref()
                    .and_then(|d| days_between(d, today)),
                journal_work_last,
                backlog_count: p.note.backlog.len(),
                backlog: p.note.backlog,
                logs,
            }
        })
        .collect();

    projects.sort_by_cached_key(sort_key);
    Collected {
        projects,
        fetch_failed,
    }
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

/// PJ 名の前に付ける印。桁を動かさないよう常に固定幅（[`MARKER_WIDTH`]）で返す。
///
/// - `!` 未反映（repo にコミットがあるのに PJ ノートのログに無い）
/// - `L` remote 未設定。バックアップが無い状態
/// - `^` 未 push コミットあり
///
/// remote が無ければ ahead は数えられないので、2文字目の `L` と `^` は排他になる。
/// 全角文字は端末によって幅の解釈が割れるので ASCII だけを使う。
fn markers(p: &PjProject) -> String {
    let unreported = if p.unreported { '!' } else { ' ' };
    let repo = match (p.has_remote, p.ahead_count) {
        (Some(false), _) => 'L',
        (_, Some(n)) if n > 0 => '^',
        _ => ' ',
    };
    format!("{unreported}{repo}")
}

/// [`markers`] が返す印の文字数。PJ 名の列幅に足す。
const MARKER_WIDTH: usize = 2;

fn print_table(projects: &[PjProject], today: &str, fetched: bool, fetch_failed: &[String]) {
    println!(
        "\x1b[1mPJ状態  {}  ({}件)\x1b[0m",
        today,
        projects.len()
    );
    println!();

    let idx_w = projects.len().to_string().len().max(1);
    let name_w = projects
        .iter()
        .map(|p| UnicodeWidthStr::width(p.name.as_str()) + MARKER_WIDTH)
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
        "  {}  {}  {}  {}  {}  {}  {}  次の予定",
        pad_display("#", idx_w),
        pad_display("PJ", name_w),
        pad_display("ログ", days_w),
        pad_display("repo", days_w),
        pad_display("実働", days_w),
        pad_display("言及", days_w),
        pad_display("状態", health_w),
    );

    // 罫線の長さを合わせるため、装飾なしの行を先に組んでから最大幅を測る
    let rows: Vec<(String, String)> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name_cell = pad_display(&format!("{}{}", markers(p), p.name), name_w);
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
                "  {}  {}  {}  {}  {}  {}  {}  ",
                pad_display_right(&(i + 1).to_string(), idx_w),
                name_cell,
                pad_display(&days_cell(p.log_days), days_w),
                pad_display(&days_cell(p.repo_days), days_w),
                pad_display(&days_cell(p.journal_work_days), days_w),
                pad_display(&days_cell(p.journal_days), days_w),
                pad_display(health_label(p.health), health_w),
            );
            let prefix_colored = format!(
                "  {}  {}  {}  {}  {}  {}  {}  ",
                pad_display_right(&(i + 1).to_string(), idx_w),
                name_colored,
                pad_display(&days_cell(p.log_days), days_w),
                pad_display(&days_cell(p.repo_days), days_w),
                pad_display(&days_cell(p.journal_work_days), days_w),
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
    let unpushed = projects
        .iter()
        .filter(|p| p.ahead_count.is_some_and(|n| n > 0))
        .count();
    let no_remote = projects
        .iter()
        .filter(|p| p.has_remote == Some(false))
        .count();

    println!();
    println!(
        "要clarify {unclarified}件 / NA不在 {no_next}件 / 未反映 {unreported}件 / 未push {unpushed}件 / remote無し {no_remote}件"
    );
    println!(
        "\x1b[2m  ログ = PJノートの最終ログ / repo = リポジトリ最終コミット / 実働 = journal で実際に手を動かした日 / 言及 = journal で触れられた日\x1b[0m"
    );
    println!(
        "\x1b[2m  ! = 未反映（repo にコミットがあるのに PJノートのログに無い）\x1b[0m"
    );
    println!(
        "\x1b[2m  ^ = 未pushコミットあり / L = remote 未設定（リモートにバックアップが無い）\x1b[0m"
    );

    if !fetched {
        println!(
            "\x1b[2m  fetch していないため repo はローカルの clone 基準（古い可能性あり）\x1b[0m"
        );
    }
    for repo in fetch_failed {
        println!("\x1b[33m  警告: fetch できませんでした: {repo}\x1b[0m");
    }
}

/// `taski pj` の本体。
pub fn run(
    base_dir: PathBuf,
    format: Option<String>,
    status: Option<String>,
    all: bool,
    today: Option<String>,
    no_fetch: bool,
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

    // fetch でネットワークを使う前に引数を検証しておく
    let as_json = match format.as_deref() {
        Some("json") => true,
        Some("table") | None => false,
        Some(other) => {
            eprintln!("エラー: 未対応のフォーマットです: {other}");
            process::exit(1);
        }
    };

    let collected = collect_projects(&base_dir, &statuses, &today, !no_fetch);

    if as_json {
        let output = PjOutput {
            generated: today,
            fetched: !no_fetch,
            fetch_failed: collected.fetch_failed,
            projects: collected.projects,
        };
        let json = serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
            eprintln!("エラー: JSON変換に失敗しました: {e}");
            process::exit(1);
        });
        println!("{json}");
    } else {
        if collected.projects.is_empty() {
            println!("該当する PJ がありません");
            return;
        }
        print_table(
            &collected.projects,
            &today,
            !no_fetch,
            &collected.fetch_failed,
        );
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
            repo_abs: None,
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
            has_remote: None,
            ahead_count: None,
            journal_last: None,
            journal_days: None,
            journal_work_last: None,
            journal_work_days: None,
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

    #[test]
    fn test_is_future() {
        assert!(is_future("2026-08-02", "2026-08-01"));
        // 基準日当日は「未来」ではない
        assert!(!is_future("2026-08-01", "2026-08-01"));
        assert!(!is_future("2026-07-31", "2026-08-01"));
    }

    // --- git log parsing ---

    #[test]
    fn test_parse_git_name_only_keeps_newest() {
        let output = "\u{0}2026-08-01\n\nnote/A.md\nnote/B.md\n\u{0}2026-07-20\n\nnote/A.md\nnote/C.md\n";
        let map = parse_git_name_only(output, "2026-08-02");
        assert_eq!(map.get("note/A.md").map(|s| s.as_str()), Some("2026-08-01"));
        assert_eq!(map.get("note/B.md").map(|s| s.as_str()), Some("2026-08-01"));
        assert_eq!(map.get("note/C.md").map(|s| s.as_str()), Some("2026-07-20"));
    }

    #[test]
    fn test_parse_git_name_only_skips_commits_after_today() {
        // 基準日より後のコミットは無かったことにするので、
        // A は 07-20 まで巻き戻り、そこにしか出てこない B は消える
        let output = "\u{0}2026-08-01\n\nnote/A.md\nnote/B.md\n\u{0}2026-07-20\n\nnote/A.md\n";
        let map = parse_git_name_only(output, "2026-07-25");
        assert_eq!(map.get("note/A.md").map(|s| s.as_str()), Some("2026-07-20"));
        assert_eq!(map.get("note/B.md"), None);
    }

    #[test]
    fn test_parse_git_name_only_handles_japanese_paths() {
        let output = "\u{0}2026-08-01\n\nnote/漫画制作エディタ.md\n";
        let map = parse_git_name_only(output, "2026-08-02");
        assert_eq!(
            map.get("note/漫画制作エディタ.md").map(|s| s.as_str()),
            Some("2026-08-01")
        );
    }

    #[test]
    fn test_parse_git_name_only_empty() {
        assert!(parse_git_name_only("", "2026-08-02").is_empty());
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
        // プロセスの HOME を書き換えると他のテストに漏れるので引数で渡す
        let home = PathBuf::from("/home/test");
        assert_eq!(
            expand_home_with("~/workspace/x", Some(&home)),
            PathBuf::from("/home/test/workspace/x")
        );
        assert_eq!(
            expand_home_with("/abs/path", Some(&home)),
            PathBuf::from("/abs/path")
        );
    }

    #[test]
    fn test_expand_home_without_home() {
        // HOME が無い環境では `~/` を展開せずそのまま扱う
        assert_eq!(
            expand_home_with("~/workspace/x", None),
            PathBuf::from("~/workspace/x")
        );
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
        list.sort_by_cached_key(sort_key);
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
        list.sort_by_cached_key(sort_key);
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
        list.sort_by_cached_key(sort_key);
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

    // --- markers ---

    #[test]
    fn test_markers_none() {
        let mut p = project("A");
        p.has_remote = Some(true);
        p.ahead_count = Some(0);
        assert_eq!(markers(&p), "  ");
    }

    #[test]
    fn test_markers_unreported() {
        let mut p = project("A");
        p.unreported = true;
        assert_eq!(markers(&p), "! ");
    }

    #[test]
    fn test_markers_no_remote() {
        let mut p = project("A");
        p.has_remote = Some(false);
        assert_eq!(markers(&p), " L");
    }

    #[test]
    fn test_markers_unpushed() {
        let mut p = project("A");
        p.has_remote = Some(true);
        p.ahead_count = Some(3);
        assert_eq!(markers(&p), " ^");
    }

    #[test]
    fn test_markers_combined() {
        let mut p = project("A");
        p.unreported = true;
        p.has_remote = Some(true);
        p.ahead_count = Some(2);
        assert_eq!(markers(&p), "!^");
    }

    #[test]
    fn test_markers_without_repo_are_blank() {
        // repo: を持たない PJ に「remote 無し」の印を付けない（バックアップすべき実体が無い）
        let p = project("A");
        assert_eq!(p.has_remote, None);
        assert_eq!(markers(&p), "  ");
    }

    #[test]
    fn test_markers_width_is_fixed() {
        // 印の有無で桁がずれないこと
        let plain = project("A");
        let mut marked = project("A");
        marked.unreported = true;
        marked.has_remote = Some(false);
        assert_eq!(markers(&plain).chars().count(), MARKER_WIDTH);
        assert_eq!(markers(&marked).chars().count(), MARKER_WIDTH);
    }
}
