//! `taski schedule` の統合テスト。
//!
//! `list` と同じく、主眼は「構造化出力は常にパースできる」こと。
//! 予定が 1 件も無い日は休日などでごく普通に起きるので、そこで日本語のメッセージを
//! 返すと呼び出し側は正常系でパースに失敗する。

mod common;

use common::{make_empty_taski, write_md, TempHome};
use std::path::Path;
use std::process::Command;

struct Output {
    success: bool,
    stdout: String,
}

fn run_schedule(home: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_taski"))
        .env("HOME", home)
        .arg("schedule")
        .args(args)
        .output()
        .expect("taski を実行できません");
    Output {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
    }
}

/// 時刻付きのログを 1 件持つタスクを置く。
fn write_scheduled_task(home: &Path) {
    write_md(
        home,
        "tasks.md",
        "# tasks\n\n- [ ] API設計レビュー\n    - 2026-04-11 10:00-11:00: エンドポイント設計の確認\n",
    );
}

#[test]
fn test_no_entries_returns_empty_json() {
    let home = TempHome::new("schedule-no-entries-json");
    let root = home.path();
    write_scheduled_task(root);

    // 予定のある日とは別の日を指定する
    let out = run_schedule(root, &["--date", "2026-04-12", "--format", "json"]);
    assert!(out.success, "終了コードは 0 のままであるべき");
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("0件でも JSON を返すべき");
    assert_eq!(json.as_array().expect("配列であるべき").len(), 0);
}

#[test]
fn test_no_entries_returns_empty_yaml() {
    let home = TempHome::new("schedule-no-entries-yaml");
    let root = home.path();
    write_scheduled_task(root);

    let out = run_schedule(root, &["--date", "2026-04-12", "--format", "yaml"]);
    assert!(out.success);
    assert_eq!(out.stdout.trim(), "[]", "0件でも空の YAML 配列を返すべき");
}

#[test]
fn test_no_markdown_files_returns_empty_json() {
    let home = TempHome::new("schedule-no-files");
    let root = home.path();
    make_empty_taski(root);

    let out = run_schedule(root, &["--date", "2026-04-12", "--format", "json"]);
    assert!(out.success);
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("ファイルが無くても JSON を返すべき");
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[test]
fn test_no_entries_text_output_keeps_message() {
    let home = TempHome::new("schedule-no-entries-text");
    let root = home.path();
    write_scheduled_task(root);

    // 人間向けの表示は従来どおりメッセージを出す
    let out = run_schedule(root, &["--date", "2026-04-12"]);
    assert!(out.success);
    assert!(
        out.stdout.contains("2026-04-12 のスケジュールはありません"),
        "テキスト表示ではメッセージを出すべき:\n{}",
        out.stdout
    );
}

#[test]
fn test_entries_are_returned() {
    let home = TempHome::new("schedule-entries");
    let root = home.path();
    write_scheduled_task(root);

    let out = run_schedule(root, &["--date", "2026-04-11", "--format", "json"]);
    assert!(out.success);
    let json: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entries = json.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["taskText"], "API設計レビュー");
    assert_eq!(entries[0]["time"], "10:00");
    assert_eq!(entries[0]["endTime"], "11:00");
}

#[test]
fn test_unsupported_format_fails_even_with_no_entries() {
    let home = TempHome::new("schedule-bad-format");
    let root = home.path();
    write_scheduled_task(root);

    // 0件でもフォーマットの誤りは黙って通さない
    let out = run_schedule(root, &["--date", "2026-04-12", "--format", "csv"]);
    assert!(!out.success, "未対応フォーマットは失敗すべき");
}
