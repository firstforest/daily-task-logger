//! `taski pj` の統合テスト。
//!
//! 未反映検出（`unreported`）は git のコミット日と PJ ノートのログ日を突き合わせるので、
//! 実際に git リポジトリを作って端から端まで確かめる。
//! 判定が厳密大なり（`repo_last > log_last`）である以上、
//! 「同日のコミットは反映済み」という境界の扱いがフラグと件数で食い違いやすい。

mod common;

use common::TempHome;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// `$HOME/taski/journal/<年>/<月>/<日付>.md` に journal を書く。
fn write_journal(home: &Path, date: &str, body: &str) {
    let dir = home
        .join("taski")
        .join("journal")
        .join(&date[0..4])
        .join(&date[5..7]);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{date}.md")), body).unwrap();
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
    // 印は「未反映」「repo の状態」の2文字。remote を持たないリポジトリなので `L` が付く
    assert!(
        stdout.contains("!L未反映PJ"),
        "未反映マーカーが無い:\n{stdout}"
    );
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
        &["clone", &origin.to_string_lossy(), &clone.to_string_lossy()],
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
    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-08-01"],
    );
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
    git(
        &taski,
        &["commit", "-m", "初回"],
        Some("2026-07-30T12:00:00+09:00"),
    );

    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-07-25"],
    );
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
    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-08-02"],
    );
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
    // 特定の文字列（`-7d`）ではなく日数セルの形で見る。書式が変わっても素通りしないように。
    let negative_days = Regex::new(r"-\d+d").unwrap();
    assert!(
        !negative_days.is_match(&stdout),
        "負の日数が出ている:\n{stdout}"
    );
    // 基準日時点ではログもコミットも無いので `-` になる
    assert!(
        stdout.contains("未来コミットPJ"),
        "PJ が出ていない:\n{stdout}"
    );
    assert!(
        stdout.contains("未反映 0件"),
        "未反映が数えられている:\n{stdout}"
    );
}

/// journal の「言及」と「実働」を分けて出すこと。
///
/// 言及だけを見ると `## 今日の候補` に載っただけの PJ が「動いている」ように見え、
/// 未反映検出（`log_last < journal_last`）が構造的に誤検出する。
#[test]
fn test_journal_work_is_separate_from_mention() {
    let home = TempHome::new("journal-work");
    let root = home.path();

    for name in ["完了PJ", "時刻ログPJ", "言及だけPJ"] {
        write_note(
            root,
            name,
            &format!("---\nproject: active\n---\n# {name}\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n"),
        );
    }

    write_journal(
        root,
        "2026-07-20",
        "# 2026-07-20\n\n- [x] [[完了PJ]] のカードを作った\n",
    );
    write_journal(
        root,
        "2026-07-22",
        "# 2026-07-22\n\n- [ ] [[時刻ログPJ]] を進める\n    - 2026-07-22 10:00-11:30: 下書きまで\n",
    );
    // 3件とも候補には載っている（＝言及はある）が、ここで動いたのは1件も無い
    write_journal(
        root,
        "2026-07-28",
        "# 2026-07-28\n\n## 今日の候補\n\n- [ ] [[完了PJ]] の続き\n- [ ] [[時刻ログPJ]] の続き\n- [ ] [[言及だけPJ]] を始める\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);

    let done = find(&json, "完了PJ");
    assert_eq!(done["journal_last"], "2026-07-28", "言及は候補に載った日");
    assert_eq!(done["journal_work_last"], "2026-07-20", "実働は完了した日");
    assert_eq!(done["journal_work_days"], 12);

    let logged = find(&json, "時刻ログPJ");
    assert_eq!(logged["journal_last"], "2026-07-28");
    assert_eq!(
        logged["journal_work_last"], "2026-07-22",
        "時刻付きログがあれば未完了でも実働"
    );

    // 候補に載っただけの PJ は実働なし。ここが null にならないと毎朝の誤検出が残る
    let mentioned = find(&json, "言及だけPJ");
    assert_eq!(mentioned["journal_last"], "2026-07-28");
    assert_eq!(
        mentioned["journal_work_last"],
        serde_json::Value::Null,
        "候補に載っただけを実働と数えてはいけない"
    );
    assert_eq!(mentioned["journal_work_days"], serde_json::Value::Null);
}

/// PJ ノートを `note/` 直下だけでなく `note/**` から拾うこと（docs/design.md G-6）。
///
/// 走査範囲が非対称だったころは `note/sub/名前.md` が「Wiki リンクとしては開けるが
/// PJ としては拾われない」状態だった。
#[test]
fn test_pj_notes_are_found_in_subdirectories() {
    let home = TempHome::new("note-subdir");
    let root = home.path();

    let body = "---\nproject: active\n---\n# n\n\n## 次の予定\n\n- [ ] やる（30分・@PC）\n";
    write_note(root, "直下PJ", body);

    let sub = root.join("taski").join("note").join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("配下PJ.md"), body).unwrap();

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let names: Vec<&str> = json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();

    assert!(names.contains(&"直下PJ"));
    assert!(names.contains(&"配下PJ"), "note/ 配下のサブディレクトリも拾うこと");
    // path は taski home からの相対パスで出る
    assert_eq!(find(&json, "配下PJ")["path"], "note/sub/配下PJ.md");
}

/// 照合キー（空白 → `_`）を参照側と PJ 名側の両方に掛けること（docs/domain.md §4）。
///
/// `#タグ` に空白を書けない以上、突き合わせはキーを経由するしかない。片側だけに
/// 掛けていると、ノート名に `_` を使った PJ が `[[名前 空白あり]]` で引けない。
#[test]
fn test_match_key_is_applied_to_both_sides() {
    let home = TempHome::new("match-key");
    let root = home.path();

    write_note(
        root,
        "在庫 管理",
        "---\nproject: active\n---\n# 在庫 管理\n\n## 次の予定\n\n- [ ] やる（30分・@PC）\n",
    );
    write_note(
        root,
        "発注_フロー",
        "---\nproject: active\n---\n# 発注_フロー\n\n## 次の予定\n\n- [ ] やる（30分・@PC）\n",
    );

    // 空白入りのノートをタグ表記で、`_` 入りのノートを空白表記の Wiki リンクで参照する
    write_journal(
        root,
        "2026-07-20",
        "# 2026-07-20\n\n- [x] #在庫_管理 の棚卸し\n- [x] [[発注 フロー]] を直した\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);

    let stock = find(&json, "在庫 管理");
    assert_eq!(stock["journal_last"], "2026-07-20");
    assert_eq!(
        stock["journal_work_last"], "2026-07-20",
        "空白入りの PJ 名は `#タグ` 表記でも当たること"
    );

    let order = find(&json, "発注_フロー");
    assert_eq!(order["journal_last"], "2026-07-20");
    assert_eq!(
        order["journal_work_last"], "2026-07-20",
        "`_` 入りの PJ 名は `[[名前]]` の空白表記でも当たること"
    );
}

/// 実働日は「新しい journal で最初に見つかった日」ではなく最大値を採ること。
///
/// 時刻付きログは自分の日付を持つので、新しい journal に前日ぶんの作業を
/// 書き足すと、降順走査で最初に当たる日付が最大とは限らない。
#[test]
fn test_journal_work_takes_max_not_first_hit() {
    let home = TempHome::new("journal-work-max");
    let root = home.path();

    write_note(
        root,
        "遡及PJ",
        "---\nproject: active\n---\n# 遡及PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    // 新しい journal には「前に少しやった分」の記録だけ（日付は 07-25）
    write_journal(
        root,
        "2026-07-30",
        "# 2026-07-30\n\n- [ ] [[遡及PJ]] の続き\n    - 2026-07-25 10:00: 前に少しやった分を記録\n",
    );
    // 実際に最後に手が動いたのは 07-28
    write_journal(
        root,
        "2026-07-28",
        "# 2026-07-28\n\n- [x] [[遡及PJ]] のカードを作った\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "遡及PJ");
    assert_eq!(p["journal_last"], "2026-07-30");
    assert_eq!(
        p["journal_work_last"], "2026-07-28",
        "古い journal にある新しい実働日を取り逃してはいけない"
    );
    assert_eq!(p["journal_work_days"], 4);
}

/// 言及と実働で参照の拾い方が揃っていること。
///
/// 実働は言及の部分集合なので、「実働はあるのに言及が null」は成り立たない。
#[test]
fn test_journal_mention_matches_work_link_normalization() {
    let home = TempHome::new("journal-link-norm");
    let root = home.path();

    write_note(
        root,
        "正規化PJ",
        "---\nproject: active\n---\n# 正規化PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    // `.md` 付きのリンク。実働側は正規化して拾うので言及側も拾えなければならない
    write_journal(
        root,
        "2026-07-30",
        "# 2026-07-30\n\n- [x] [[正規化PJ.md]] のカードを作った\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "正規化PJ");
    assert_eq!(p["journal_work_last"], "2026-07-30");
    assert_eq!(
        p["journal_last"], "2026-07-30",
        "実働があるのに言及が null になってはいけない"
    );
}

/// コードフェンスの中に書いた参照が言及にならないこと。
///
/// フェンス内は全解析から除外する（docs/syntax.md §2.3）。記法の例を journal に
/// 貼っただけの PJ が「言及された」ことになると、停滞の判定が効かなくなる。
#[test]
fn test_journal_mention_ignores_code_block() {
    let home = TempHome::new("journal-fence");
    let root = home.path();

    write_note(
        root,
        "フェンスPJ",
        "---\nproject: active\n---\n# フェンスPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    write_note(
        root,
        "本文PJ",
        "---\nproject: active\n---\n# 本文PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    write_journal(
        root,
        "2026-07-30",
        "# 2026-07-30\n\n\
         書き方の例:\n\n\
         ```markdown\n\
         - [ ] [[フェンスPJ]] を進める\n\
         ```\n\n\
         - [ ] [[本文PJ]] を進める\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);

    let fenced = find(&json, "フェンスPJ");
    assert_eq!(
        fenced["journal_last"],
        serde_json::Value::Null,
        "フェンス内の参照を言及として拾ってはいけない"
    );

    let plain = find(&json, "本文PJ");
    assert_eq!(
        plain["journal_last"], "2026-07-30",
        "フェンスを閉じた後の参照は通常どおり拾う"
    );
}

/// 候補に並べた PJ が、別セクションの無関係な時刻メモで実働扱いにならないこと。
#[test]
fn test_candidate_list_is_not_work_despite_timed_note() {
    let home = TempHome::new("journal-candidate");
    let root = home.path();

    write_note(
        root,
        "候補だけPJ",
        "---\nproject: active\n---\n# 候補だけPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    // 「今日の候補」＋別セクションの時刻メモ、という journal の普通の形
    write_journal(
        root,
        "2026-07-30",
        "# 2026-07-30\n\n\
         ## 今日の候補\n\n\
         - [ ] [[候補だけPJ]] を始める\n\n\
         ## 記録\n\n\
         - 定例ミーティング\n\
         \x20   - 2026-07-30 10:00-11:00: 進捗共有\n",
    );

    let json = run_pj(root, &["--format", "json", "--today", "2026-08-01"]);
    let p = find(&json, "候補だけPJ");
    assert_eq!(p["journal_last"], "2026-07-30");
    assert_eq!(
        p["journal_work_last"],
        serde_json::Value::Null,
        "別セクションの時刻メモを実働と取り違えてはいけない"
    );
}

/// `repo:` の展開済み絶対パスを出すこと（利用側に `~` 展開を再実装させない）。
#[test]
fn test_repo_abs_is_expanded() {
    let home = TempHome::new("repo-abs");
    let root = home.path();
    let repo = make_repo(root, "repo-abs-target", &["2026-07-26"]);

    // front matter には `~/` 形式の生値を書く
    write_note(
        root,
        "展開PJ",
        "---\nproject: active\nrepo: ~/repo-abs-target\n---\n# 展開PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    write_note(
        root,
        "リポジトリ無しPJ",
        "---\nproject: active\nrepo: ~/does-not-exist-98765\n---\n# リポジトリ無しPJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );
    write_note(
        root,
        "repo未指定PJ",
        "---\nproject: active\n---\n# repo未指定PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-08-01"],
    );

    let expanded = find(&json, "展開PJ");
    // 生値はそのまま残し、展開済みパスを別フィールドで足す
    assert_eq!(expanded["repo"], "~/repo-abs-target");
    let expected = fs::canonicalize(&repo).unwrap().to_string_lossy().to_string();
    assert_eq!(expanded["repo_abs"], expected);

    // 存在しないディレクトリは null（前方一致で誤爆させない）
    let missing = find(&json, "リポジトリ無しPJ");
    assert_eq!(missing["repo"], "~/does-not-exist-98765");
    assert_eq!(missing["repo_abs"], serde_json::Value::Null);

    assert_eq!(find(&json, "repo未指定PJ")["repo_abs"], serde_json::Value::Null);
}

/// remote 未設定・未 push コミットを検出すること。
#[test]
fn test_has_remote_and_ahead_count() {
    let home = TempHome::new("ahead");
    let root = home.path();

    // remote を持たないローカル専用リポジトリ
    make_repo(root, "repo-local", &["2026-07-20"]);

    // 全部 push 済みの clone と、ローカルにだけコミットがある clone
    let origin = make_repo(root, "repo-origin", &["2026-07-20"]);
    let synced = root.join("repo-synced");
    let ahead = root.join("repo-ahead");
    for clone in [&synced, &ahead] {
        git(
            root,
            &["clone", &origin.to_string_lossy(), &clone.to_string_lossy()],
            None,
        );
    }
    commit_file(
        &ahead,
        1,
        Some("2026-07-25T12:00:00+09:00"),
        Some("2026-07-25T12:00:00+09:00"),
    );
    commit_file(
        &ahead,
        2,
        Some("2026-07-26T12:00:00+09:00"),
        Some("2026-07-26T12:00:00+09:00"),
    );

    for (name, repo) in [
        ("ローカル専用PJ", Some("~/repo-local")),
        ("同期済みPJ", Some("~/repo-synced")),
        ("未pushPJ", Some("~/repo-ahead")),
        ("repo無しPJ", None),
    ] {
        let front = match repo {
            Some(repo) => format!("---\nproject: active\nrepo: {repo}\n---\n"),
            None => "---\nproject: active\n---\n".to_string(),
        };
        write_note(
            root,
            name,
            &format!("{front}# {name}\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n"),
        );
    }

    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-08-01"],
    );

    // remote が無い = GitHub にバックアップが無い。ahead は「push 済みかどうか」の
    // 区別自体が無いので null
    let local = find(&json, "ローカル専用PJ");
    assert_eq!(local["has_remote"], false);
    assert_eq!(local["ahead_count"], serde_json::Value::Null);

    let synced = find(&json, "同期済みPJ");
    assert_eq!(synced["has_remote"], true);
    assert_eq!(synced["ahead_count"], 0);

    let ahead = find(&json, "未pushPJ");
    assert_eq!(ahead["has_remote"], true);
    assert_eq!(ahead["ahead_count"], 2);

    // repo: を持たない PJ に「remote 無し」と言わない（バックアップすべき実体が無い）
    let none = find(&json, "repo無しPJ");
    assert_eq!(none["has_remote"], serde_json::Value::Null);
    assert_eq!(none["ahead_count"], serde_json::Value::Null);
}

/// 基準日より後の未 push コミットは数えないこと（`--today` で過去を振り返る場合）。
#[test]
fn test_ahead_count_respects_today() {
    let home = TempHome::new("ahead-today");
    let root = home.path();

    let origin = make_repo(root, "repo-origin-past", &["2026-07-20"]);
    let clone = root.join("repo-clone-past");
    git(
        root,
        &["clone", &origin.to_string_lossy(), &clone.to_string_lossy()],
        None,
    );
    commit_file(
        &clone,
        1,
        Some("2026-07-31T12:00:00+09:00"),
        Some("2026-07-31T12:00:00+09:00"),
    );

    write_note(
        root,
        "過去基準PJ",
        "---\nproject: active\nrepo: ~/repo-clone-past\n---\n# 過去基準PJ\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n",
    );

    // 07-25 時点では 07-31 のコミットはまだ無い
    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-07-25"],
    );
    assert_eq!(find(&json, "過去基準PJ")["ahead_count"], 0);

    let json = run_pj(
        root,
        &["--format", "json", "--no-fetch", "--today", "2026-08-01"],
    );
    assert_eq!(find(&json, "過去基準PJ")["ahead_count"], 1);
}

/// table に remote 未設定・未 push の印が出ること。
#[test]
fn test_table_marks_repo_state() {
    let home = TempHome::new("table-repo-state");
    let root = home.path();

    make_repo(root, "repo-local-mark", &["2026-07-20"]);
    let origin = make_repo(root, "repo-origin-mark", &["2026-07-20"]);
    let ahead = root.join("repo-ahead-mark");
    git(
        root,
        &["clone", &origin.to_string_lossy(), &ahead.to_string_lossy()],
        None,
    );
    commit_file(
        &ahead,
        1,
        Some("2026-07-25T12:00:00+09:00"),
        Some("2026-07-25T12:00:00+09:00"),
    );

    for (name, repo) in [
        ("ローカルのみPJ", "~/repo-local-mark"),
        ("未push印PJ", "~/repo-ahead-mark"),
    ] {
        write_note(
            root,
            name,
            &format!(
                "---\nproject: active\nrepo: {repo}\n---\n# {name}\n\n## 次の予定\n\n- [ ] やる（30分・軽・@PC）\n\n## ログ\n\n- 2026-07-30: ここまで\n"
            ),
        );
    }

    let out = Command::new(env!("CARGO_BIN_EXE_taski"))
        .env("HOME", root)
        .args(["pj", "--no-fetch", "--today", "2026-08-01"])
        .output()
        .expect("taski を実行できません");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains(" Lローカルのみ"),
        "remote 未設定の印が無い:\n{stdout}"
    );
    assert!(stdout.contains(" ^未push印PJ"), "未pushの印が無い:\n{stdout}");
    assert!(
        stdout.contains("未push 1件") && stdout.contains("remote無し 1件"),
        "集計が合わない:\n{stdout}"
    );
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
