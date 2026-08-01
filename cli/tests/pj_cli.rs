//! `taski pj` の統合テスト。
//!
//! 未反映検出（`unreported`）は git のコミット日と PJ ノートのログ日を突き合わせるので、
//! 実際に git リポジトリを作って端から端まで確かめる。
//! 判定が厳密大なり（`repo_last > log_last`）である以上、
//! 「同日のコミットは反映済み」という境界の扱いがフラグと件数で食い違いやすい。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// テスト用の一時ディレクトリ。Drop で消す。
struct TempHome(PathBuf);

impl TempHome {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("taski-pj-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        TempHome(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(repo: &Path, args: &[&str], date: Option<&str>) {
    git_dates(repo, args, date, date);
}

/// author date と committer date を別々に指定して git を叩く。
/// 両者がずれるのは rebase / cherry-pick / `--amend` の後で、実運用では珍しくない。
fn git_dates(repo: &Path, args: &[&str], author: Option<&str>, committer: Option<&str>) {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args);
    if let Some(date) = author {
        cmd.env("GIT_AUTHOR_DATE", date);
    }
    if let Some(date) = committer {
        cmd.env("GIT_COMMITTER_DATE", date);
    }
    let out = cmd.output().expect("git を実行できません");
    assert!(
        out.status.success(),
        "git {args:?} が失敗しました: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 指定した日付ぶんのコミットを持つリポジトリを作る。時刻は日本時間の正午。
fn make_repo(root: &Path, name: &str, dates: &[&str]) -> PathBuf {
    let stamps: Vec<String> = dates.iter().map(|d| format!("{d}T12:00:00+09:00")).collect();
    let refs: Vec<&str> = stamps.iter().map(|s| s.as_str()).collect();
    make_repo_at(root, name, &refs)
}

/// タイムスタンプ（オフセット込み）をそのまま指定してリポジトリを作る。
fn make_repo_at(root: &Path, name: &str, stamps: &[&str]) -> PathBuf {
    let repo = root.join(name);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"], None);
    for (i, stamp) in stamps.iter().enumerate() {
        commit_file(&repo, i, Some(stamp), Some(stamp));
    }
    repo
}

fn commit_file(repo: &Path, i: usize, author: Option<&str>, committer: Option<&str>) {
    fs::write(repo.join(format!("f{i}.txt")), format!("{i}")).unwrap();
    git(repo, &["add", "."], None);
    git_dates(
        repo,
        &["commit", "-m", &format!("commit {i}")],
        author,
        committer,
    );
}

/// 現在の HEAD のコミットハッシュ。fetch が作業ツリーを動かしていないことの確認に使う。
fn rev_parse(repo: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git を実行できません");
    assert!(out.status.success(), "rev-parse に失敗しました");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write_note(home: &Path, name: &str, body: &str) {
    let note_dir = home.join("taski").join("note");
    fs::create_dir_all(&note_dir).unwrap();
    fs::write(note_dir.join(format!("{name}.md")), body).unwrap();
}

/// `taski pj --format json` を実行して JSON を返す。
fn run_pj(home: &Path, args: &[&str]) -> serde_json::Value {
    run_pj_tz(home, None, args)
}

/// タイムゾーンを指定して `taski pj --format json` を実行する。
fn run_pj_tz(home: &Path, tz: Option<&str>, args: &[&str]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_taski"));
    cmd.env("HOME", home).arg("pj").args(args);
    if let Some(tz) = tz {
        cmd.env("TZ", tz);
    }
    let out = cmd.output().expect("taski を実行できません");
    assert!(
        out.status.success(),
        "taski pj が失敗しました: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("JSON をパースできません")
}

fn find<'a>(json: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("{name} が見つかりません"))
}

#[test]
fn test_unreported_detection_end_to_end() {
    let home = TempHome::new("unreported");
    let root = home.path();

    // repo にコミットがあるのに PJ ノートのログが古い → 未反映
    let behind = make_repo(root, "repo-behind", &["2026-07-20", "2026-07-25", "2026-07-26"]);
    // ログとリポジトリの最終日が同じ → 未反映ではない（境界）
    let same_day = make_repo(root, "repo-same-day", &["2026-07-20"]);

    write_note(
        root,
        "遅れているPJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 遅れているPJ\n\n## 次の予定\n\n- [ ] 続きをやる（30分・重・@PC）\n\n## ログ\n\n- 2026-07-20: ここまでやった\n",
            behind.display()
        ),
    );
    write_note(
        root,
        "同日PJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 同日PJ\n\n## 次の予定\n\n- [ ] 続きをやる（30分・重・@PC）\n\n## ログ\n\n- 2026-07-20: 当日中に反映済み\n",
            same_day.display()
        ),
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);

    let behind = find(&json, "遅れているPJ");
    assert_eq!(behind["repo_last"], "2026-07-26");
    assert_eq!(behind["log_last"], "2026-07-20");
    assert_eq!(behind["unreported"], true);
    // 2026-07-20 当日のコミットは反映済みなので、数えるのは 07-25 と 07-26 の2件
    assert_eq!(behind["unreported_count"], 2);
    assert_eq!(behind["repo_days"], 6);
    assert_eq!(behind["log_days"], 12);

    let same = find(&json, "同日PJ");
    assert_eq!(same["repo_last"], "2026-07-20");
    assert_eq!(same["unreported"], false);
    // フラグが false のとき件数も必ず 0（`--since` を使うとここが 1 になって食い違う）
    assert_eq!(same["unreported_count"], 0);
}

#[test]
fn test_unreported_count_is_timezone_independent() {
    let home = TempHome::new("tz");
    let root = home.path();

    // 日本時間の 07-27 00:30 = UTC の 07-26 15:30。ログは 07-26 なので
    // 「repo_last(07-27) > log_last(07-26) で1件未反映」が TZ によらず正しい答え。
    // 件数を git の `--after`（committer date をローカル TZ で解釈）に数えさせると、
    // TZ=UTC では 0 件になってフラグと食い違う。
    let repo = make_repo_at(
        root,
        "repo-tz",
        &["2026-07-20T12:00:00+09:00", "2026-07-27T00:30:00+09:00"],
    );

    write_note(
        root,
        "TZ検証PJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# TZ検証PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-07-26: ここまで\n",
            repo.display()
        ),
    );

    for tz in ["UTC", "Asia/Tokyo", "America/Los_Angeles"] {
        let json = run_pj_tz(root, Some(tz), &["--format", "json", "--today", "2026-08-01"]);
        let p = find(&json, "TZ検証PJ");
        assert_eq!(p["repo_last"], "2026-07-27", "TZ={tz}");
        assert_eq!(p["unreported"], true, "TZ={tz}");
        assert_eq!(p["unreported_count"], 1, "TZ={tz} で件数が食い違う");
    }
}

#[test]
fn test_repo_last_uses_newest_date_after_rebase() {
    let home = TempHome::new("rebase");
    let root = home.path();

    // rebase / cherry-pick 相当: author date は古いまま committer date だけ新しい。
    // `git log -1` は committer date 順の先頭を取るので、そこから author date を
    // 出すと 07-10 になり、より新しい 07-25 のコミットを取りこぼす。
    let repo = root.join("repo-rebased");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"], None);
    commit_file(&repo, 0, Some("2026-07-25T12:00:00+09:00"), Some("2026-07-25T12:00:00+09:00"));
    commit_file(&repo, 1, Some("2026-07-10T12:00:00+09:00"), Some("2026-07-30T12:00:00+09:00"));

    write_note(
        root,
        "rebase済みPJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# rebase済みPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-07-20: ここまで\n",
            repo.display()
        ),
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "rebase済みPJ");
    assert_eq!(p["repo_last"], "2026-07-25", "最新の日付を採るべき");
    assert_eq!(p["unreported"], true);
    // 07-20 より後は 07-25 の1件だけ（07-10 は含めない）
    assert_eq!(p["unreported_count"], 1);
}

#[test]
fn test_unreported_count_without_log_is_all_commits() {
    let home = TempHome::new("no-log");
    let root = home.path();

    let repo = make_repo(root, "repo-no-log", &["2026-07-20", "2026-07-25", "2026-07-26"]);

    write_note(
        root,
        "ログなしPJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# ログなしPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
            repo.display()
        ),
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "ログなしPJ");
    assert_eq!(p["log_last"], serde_json::Value::Null);
    // ログが1件も無いので全コミットが未反映。フラグと件数の整合を保つ
    assert_eq!(p["unreported"], true);
    assert_eq!(p["unreported_count"], 3);
}

#[test]
fn test_note_updated_when_taski_is_not_repo_root() {
    let home = TempHome::new("subdir");
    let root = home.path();

    write_note(
        root,
        "サブディレクトリPJ",
        "---\nproject: active\n---\n# サブディレクトリPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    // `~/taski` がリポジトリ直下ではなく、その1つ上がリポジトリルートのケース
    // （dotfiles リポジトリ配下など）。`git log --name-only` は `--relative` が無いと
    // リポジトリルート相対のパスを出すため、note/*.md と突き合わせられなくなる。
    git(root, &["init"], None);
    git(root, &["add", "."], None);
    git(root, &["commit", "-m", "初回"], Some("2026-07-30T12:00:00+09:00"));

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "サブディレクトリPJ");
    assert_eq!(p["updated"], "2026-07-30");
    assert_eq!(p["stale_days"], 2);
}

#[test]
fn test_missing_repo_is_tolerated() {
    let home = TempHome::new("missing-repo");
    let root = home.path();

    write_note(
        root,
        "リポジトリ無しPJ",
        "---\nproject: active\nrepo: ~/does-not-exist-12345\n---\n# リポジトリ無しPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "リポジトリ無しPJ");
    assert_eq!(p["repo_last"], serde_json::Value::Null);
    assert_eq!(p["unreported"], false);
    assert_eq!(p["health"], "ok");
}

#[test]
fn test_health_and_status_filter() {
    let home = TempHome::new("health");
    let root = home.path();

    write_note(
        root,
        "整ったPJ",
        "---\nproject: active\n---\n# 整ったPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    write_note(
        root,
        "未clarifyPJ",
        "---\nproject: active\n---\n# 未clarifyPJ\n\n## 次の予定\n\n- [ ] 会計の仕事をやる\n",
    );
    write_note(
        root,
        "次がないPJ",
        "---\nproject: active\n---\n# 次がないPJ\n\n## 次の予定\n\n- [x] 終わった\n\n## オープンタスク\n\n- あとでやる\n- これもやる\n",
    );
    write_note(
        root,
        "棚上げPJ",
        "---\nproject: someday\n---\n# 棚上げPJ\n\n## 次の予定\n\n- [ ] いつかやる（30分・軽・@PC）\n",
    );
    // front matter に project: が無いノートは対象外
    write_note(root, "ただのノート", "# ただのノート\n\n- [ ] タスク\n");

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let names: Vec<&str> = json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 3, "既定では active のみ: {names:?}");
    assert!(!names.contains(&"棚上げPJ"));
    assert!(!names.contains(&"ただのノート"));

    assert_eq!(find(&json, "整ったPJ")["health"], "ok");
    assert_eq!(find(&json, "未clarifyPJ")["health"], "unclarified");

    let no_next = find(&json, "次がないPJ");
    assert_eq!(no_next["health"], "no-next");
    assert_eq!(no_next["next_action"], serde_json::Value::Null);
    assert_eq!(no_next["backlog_count"], 2);

    // 次の予定が無い PJ が先頭に来る（手を入れる必要が高い順）
    assert_eq!(names[0], "次がないPJ");

    let json = run_pj(
        root,
        &["--format", "json", "--status", "active,someday", "--today", "2026-08-01"],
    );
    assert_eq!(json["projects"].as_array().unwrap().len(), 4);

    let json = run_pj(root, &["--format", "json", "--status", "someday", "--today", "2026-08-01"]);
    assert_eq!(json["projects"].as_array().unwrap().len(), 1);
    assert_eq!(find(&json, "棚上げPJ")["status"], "someday");
}

#[test]
fn test_journal_mention_and_note_updated() {
    let home = TempHome::new("journal");
    let root = home.path();
    let taski = root.join("taski");

    write_note(
        root,
        "言及されるPJ",
        "---\nproject: active\n---\n# 言及されるPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    write_note(
        root,
        "言及されないPJ",
        "---\nproject: active\n---\n# 言及されないPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    let journal_dir = taski.join("journal").join("2026").join("07");
    fs::create_dir_all(&journal_dir).unwrap();
    fs::write(
        journal_dir.join("2026-07-10.md"),
        "# 2026-07-10\n\n- [ ] [[言及されるPJ]] を進める\n",
    )
    .unwrap();
    fs::write(
        journal_dir.join("2026-07-28.md"),
        "# 2026-07-28\n\n- [ ] [[言及されるPJ]] の続き\n",
    )
    .unwrap();

    // note/ の最終更新日は git から取るので、コミットしておく
    git(&taski, &["init"], None);
    git(&taski, &["add", "."], None);
    git(&taski, &["commit", "-m", "初回"], Some("2026-07-30T12:00:00+09:00"));

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);

    let mentioned = find(&json, "言及されるPJ");
    // 新しい方の日付が採られる
    assert_eq!(mentioned["journal_last"], "2026-07-28");
    assert_eq!(mentioned["journal_days"], 4);
    assert_eq!(mentioned["updated"], "2026-07-30");
    assert_eq!(mentioned["stale_days"], 2);

    let unmentioned = find(&json, "言及されないPJ");
    assert_eq!(unmentioned["journal_last"], serde_json::Value::Null);
    assert_eq!(unmentioned["journal_days"], serde_json::Value::Null);
}

#[test]
fn test_journal_mention_by_tag() {
    let home = TempHome::new("tag");
    let root = home.path();

    // 空白を含む PJ 名はタグで書けないが、含まない名前ならタグでも拾える
    write_note(
        root,
        "永夜",
        "---\nproject: active\n---\n# 永夜\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    write_note(
        root,
        "永夜祭",
        "---\nproject: active\n---\n# 永夜祭\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    let journal_dir = root.join("taski").join("journal").join("2026").join("07");
    fs::create_dir_all(&journal_dir).unwrap();
    fs::write(
        journal_dir.join("2026-07-15.md"),
        "# 2026-07-15\n\n- [x] カードを作る #永夜\n",
    )
    .unwrap();

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    assert_eq!(find(&json, "永夜")["journal_last"], "2026-07-15");
    // 前方一致で誤爆しないこと
    assert_eq!(
        find(&json, "永夜祭")["journal_last"],
        serde_json::Value::Null
    );
}

#[test]
fn test_table_output_marks_unreported() {
    let home = TempHome::new("table");
    let root = home.path();
    let repo = make_repo(root, "repo-x", &["2026-07-26"]);

    write_note(
        root,
        "未反映PJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 未反映PJ\n\n## 次の予定\n\n- [ ] 続きをやる（30分・重・@PC）\n\n## ログ\n\n- 2026-07-01: 着手した\n",
            repo.display()
        ),
    );

    let out = Command::new(env!("CARGO_BIN_EXE_taski"))
        .env("HOME", root)
        .args(["pj", "--today", "2026-08-01"])
        .output()
        .expect("taski を実行できません");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("!未反映PJ"), "未反映マーカーが無い:\n{stdout}");
    assert!(stdout.contains("未反映 1件"), "集計が合わない:\n{stdout}");
}

#[test]
fn test_fetch_is_on_by_default_and_skips_repos_without_remote() {
    let home = TempHome::new("fetch-default");
    let root = home.path();
    let repo = make_repo(root, "repo-local-only", &["2026-07-26"]);

    write_note(
        root,
        "ローカル専用PJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# ローカル専用PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
            repo.display()
        ),
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    assert_eq!(json["fetched"], true);
    // リモートを持たないリポジトリは fetch をスキップするので失敗にはならない
    assert_eq!(json["fetch_failed"].as_array().unwrap().len(), 0);
    assert_eq!(find(&json, "ローカル専用PJ")["repo_last"], "2026-07-26");
}

/// fetch そのものが効いていること（周辺ケースではなく本命の経路）。
///
/// clone が知らないコミットが origin にある状態を作り、既定では新しいコミットを
/// 拾えること・`--no-fetch` では拾えないことを両方見る。この差が出ないなら
/// fetch は呼ばれていない（他の fetch テストは呼ばれなくても通ってしまう）。
#[test]
fn test_fetch_updates_stale_clone() {
    let home = TempHome::new("fetch-updates");
    let root = home.path();

    // origin 側にだけ 07-31 のコミットがある状態にする
    let origin = make_repo(root, "repo-origin", &["2026-07-20"]);
    let clone = root.join("repo-clone");
    git(
        root,
        &[
            "clone",
            &origin.to_string_lossy(),
            &clone.to_string_lossy(),
        ],
        None,
    );
    commit_file(
        &origin,
        99,
        Some("2026-07-31T12:00:00+09:00"),
        Some("2026-07-31T12:00:00+09:00"),
    );

    write_note(
        root,
        "古いclonePJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 古いclonePJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-07-25: ここまで\n",
            clone.display()
        ),
    );

    let head_before = rev_parse(&clone);

    // fetch しなければ clone が知っている 07-20 までしか見えない
    let json = run_pj(root, &["--format", "json", "--no-fetch", "--today", "2026-08-01"]);
    let p = find(&json, "古いclonePJ");
    assert_eq!(p["repo_last"], "2026-07-20");
    assert_eq!(p["unreported"], false);

    // 既定では fetch するので origin の 07-31 を拾い、ログ（07-25）より新しいので未反映になる
    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    assert_eq!(json["fetch_failed"].as_array().unwrap().len(), 0);
    let p = find(&json, "古いclonePJ");
    assert_eq!(p["repo_last"], "2026-07-31", "fetch が効いていない");
    assert_eq!(p["unreported"], true);
    assert_eq!(p["unreported_count"], 1);

    // fetch のみで pull はしない: HEAD も作業ツリーも触らない
    assert_eq!(rev_parse(&clone), head_before, "HEAD が動いている");
    assert!(
        !clone.join("f99.txt").exists(),
        "作業ツリーに origin のファイルが現れている"
    );
}

#[test]
fn test_no_fetch_flag() {
    let home = TempHome::new("no-fetch");
    let root = home.path();
    let repo = make_repo(root, "repo-y", &["2026-07-26"]);

    write_note(
        root,
        "PJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
            repo.display()
        ),
    );

    let json = run_pj(root, &["--format", "json", "--no-fetch", "--today", "2026-08-01"]);
    assert_eq!(json["fetched"], false);
    assert_eq!(find(&json, "PJ")["repo_last"], "2026-07-26");

    let out = Command::new(env!("CARGO_BIN_EXE_taski"))
        .env("HOME", root)
        .args(["pj", "--no-fetch", "--today", "2026-08-01"])
        .output()
        .expect("taski を実行できません");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("fetch していないため"),
        "fetch していない旨の注記が無い:\n{stdout}"
    );
}

#[test]
fn test_fetch_failure_is_reported_but_does_not_abort() {
    let home = TempHome::new("fetch-fail");
    let root = home.path();
    let repo = make_repo(root, "repo-bad-remote", &["2026-07-26"]);
    // 存在しないローカルパスを remote にすると、ネットワークを使わずに fetch が失敗する
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            &root.join("does-not-exist.git").to_string_lossy(),
        ],
        None,
    );

    write_note(
        root,
        "壊れたremotePJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 壊れたremotePJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-07-01: 着手した\n",
            repo.display()
        ),
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    assert_eq!(json["fetched"], true);
    let failed = json["fetch_failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1, "fetch 失敗が報告されていない: {failed:?}");
    assert!(failed[0].as_str().unwrap().ends_with("repo-bad-remote"));

    // fetch が失敗しても集計そのものは続行する
    let p = find(&json, "壊れたremotePJ");
    assert_eq!(p["repo_last"], "2026-07-26");
    assert_eq!(p["unreported"], true);
}

/// `--today` に過去日を渡すと、その日時点の状態を再現する。
///
/// 基準日より後のログ・コミット・言及は「まだ無い」ものとして扱う。
/// 残すと経過日数が負になり、当時存在しなかったものが `-7d` として並んでしまう。
#[test]
fn test_past_today_reproduces_that_days_state() {
    let home = TempHome::new("past-today");
    let root = home.path();
    let taski = root.join("taski");

    // 07-20 と 07-31 のコミット。基準日 07-25 の時点では 07-20 までしか無い
    let repo = make_repo(root, "repo-past", &["2026-07-20", "2026-07-31"]);

    write_note(
        root,
        "振り返りPJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 振り返りPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-08-01: 基準日より後のログ\n- 2026-07-18: 基準日より前のログ\n",
            repo.display()
        ),
    );

    let journal_dir = taski.join("journal").join("2026").join("07");
    fs::create_dir_all(&journal_dir).unwrap();
    fs::write(
        journal_dir.join("2026-07-15.md"),
        "# 2026-07-15\n\n- [ ] [[振り返りPJ]] を進める\n",
    )
    .unwrap();
    // 基準日より後の journal（先に書いた明日のぶん）は見ない
    fs::write(
        journal_dir.join("2026-07-30.md"),
        "# 2026-07-30\n\n- [ ] [[振り返りPJ]] の続き\n",
    )
    .unwrap();

    // note の最終更新日（git 基準）も基準日より後なので、07-25 時点では未コミット扱い
    git(&taski, &["init"], None);
    git(&taski, &["add", "."], None);
    git(&taski, &["commit", "-m", "初回"], Some("2026-07-30T12:00:00+09:00"));

    let json = run_pj(root, &["--format", "json", "--no-fetch", "--today", "2026-07-25"]);
    let p = find(&json, "振り返りPJ");

    assert_eq!(p["log_last"], "2026-07-18");
    assert_eq!(p["log_days"], 7);
    assert_eq!(p["repo_last"], "2026-07-20");
    assert_eq!(p["repo_days"], 5);
    assert_eq!(p["journal_last"], "2026-07-15");
    assert_eq!(p["journal_days"], 10);
    assert_eq!(p["updated"], serde_json::Value::Null);
    assert_eq!(p["stale_days"], serde_json::Value::Null);
    // 07-20 のコミットはログ（07-18）より新しいので、この時点では未反映
    assert_eq!(p["unreported"], true);
    assert_eq!(p["unreported_count"], 1);
    // 基準日より後のログは再開時のコンテキストにも出さない
    let log_dates: Vec<&str> = p["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["date"].as_str().unwrap())
        .collect();
    assert_eq!(log_dates, ["2026-07-18"]);

    for key in ["stale_days", "log_days", "repo_days", "journal_days"] {
        let v = &p[key];
        assert!(
            v.is_null() || v.as_i64().unwrap() >= 0,
            "{key} が負になっている: {v}"
        );
    }

    // 基準日を今日側に戻せば、後ろのログもコミットも見える
    let json = run_pj(root, &["--format", "json", "--no-fetch", "--today", "2026-08-02"]);
    let p = find(&json, "振り返りPJ");
    assert_eq!(p["log_last"], "2026-08-01");
    assert_eq!(p["repo_last"], "2026-07-31");
    assert_eq!(p["journal_last"], "2026-07-30");
    assert_eq!(p["updated"], "2026-07-30");
}

/// table でも過去日で負の日数が出ないこと。
#[test]
fn test_past_today_table_has_no_negative_days() {
    let home = TempHome::new("past-today-table");
    let root = home.path();
    let repo = make_repo(root, "repo-past-table", &["2026-07-31"]);

    write_note(
        root,
        "未来コミットPJ",
        &format!(
            "---\nproject: active\nrepo: {}\n---\n# 未来コミットPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-08-01: 基準日より後のログ\n",
            repo.display()
        ),
    );

    let out = Command::new(env!("CARGO_BIN_EXE_taski"))
        .env("HOME", root)
        .args(["pj", "--no-fetch", "--today", "2026-07-25"])
        .output()
        .expect("taski を実行できません");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("-7d"), "負の日数が出ている:\n{stdout}");
    // 基準日時点ではログもコミットも無いので `-` になる
    assert!(stdout.contains("未来コミットPJ"), "PJ が出ていない:\n{stdout}");
    assert!(stdout.contains("未反映 0件"), "未反映が数えられている:\n{stdout}");
}

#[test]
fn test_invalid_status_and_format_fail() {
    let home = TempHome::new("invalid");
    let root = home.path();
    write_note(
        root,
        "PJ",
        "---\nproject: active\n---\n# PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    for args in [
        vec!["pj", "--status", "paused"],
        vec!["pj", "--format", "csv"],
        vec!["pj", "--today", "2026/08/01"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_taski"))
            .env("HOME", root)
            .args(&args)
            .output()
            .expect("taski を実行できません");
        assert!(!out.status.success(), "{args:?} は失敗すべき");
    }
}
