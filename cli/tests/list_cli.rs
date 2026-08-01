//! `taski list` の統合テスト。
//!
//! 主眼は「構造化出力は常にパースできる」こと。該当0件のときだけ日本語のメッセージを
//! 返すと、呼び出し側は正常系でパースに失敗する。`project: someday` / `project: done` の
//! ノートは自動タグが付かないので、PJ 名でタグを引くと 0 件は正常系として起きる。

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
        let dir = std::env::temp_dir().join(format!(
            "taski-list-{label}-{}-{nanos}",
            std::process::id()
        ));
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

/// `$HOME/taski/<name>` に Markdown を書く。
fn write_md(home: &Path, name: &str, body: &str) {
    let path = home.join("taski").join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// `$HOME/taski` だけ作ってファイルは置かない。
fn make_empty_taski(home: &Path) {
    fs::create_dir_all(home.join("taski")).unwrap();
}

struct Output {
    success: bool,
    stdout: String,
}

fn run_list(home: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_taski"))
        .env("HOME", home)
        .arg("list")
        .args(args)
        .output()
        .expect("taski を実行できません");
    Output {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
    }
}

#[test]
fn test_tag_with_no_match_returns_empty_json() {
    let home = TempHome::new("no-match-json");
    let root = home.path();
    write_md(root, "tasks.md", "# tasks\n\n- [ ] 何かやる #work\n");

    let out = run_list(root, &["--tag", "zzz_notexist", "--format", "json"]);
    assert!(out.success, "終了コードは 0 のままであるべき");
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("0件でも JSON を返すべき");
    assert_eq!(json.as_array().expect("配列であるべき").len(), 0);
}

#[test]
fn test_tag_with_no_match_returns_empty_yaml() {
    let home = TempHome::new("no-match-yaml");
    let root = home.path();
    write_md(root, "tasks.md", "# tasks\n\n- [ ] 何かやる #work\n");

    let out = run_list(root, &["--tag", "zzz_notexist", "--format", "yaml"]);
    assert!(out.success);
    assert_eq!(out.stdout.trim(), "[]", "0件でも空の YAML 配列を返すべき");
}

#[test]
fn test_no_markdown_files_returns_empty_json() {
    let home = TempHome::new("no-files");
    let root = home.path();
    make_empty_taski(root);

    let out = run_list(root, &["--format", "json"]);
    assert!(out.success);
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("ファイルが無くても JSON を返すべき");
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[test]
fn test_no_match_text_output_keeps_message() {
    let home = TempHome::new("no-match-text");
    let root = home.path();
    write_md(root, "tasks.md", "# tasks\n\n- [ ] 何かやる #work\n");

    // 人間向けの表示は従来どおりメッセージを出す
    let out = run_list(root, &["--tag", "zzz_notexist"]);
    assert!(out.success);
    assert!(
        out.stdout.contains("該当するタグのタスクが見つかりません"),
        "テキスト表示ではメッセージを出すべき:\n{}",
        out.stdout
    );
}

#[test]
fn test_matching_tag_still_returns_tasks() {
    let home = TempHome::new("match");
    let root = home.path();
    write_md(
        root,
        "tasks.md",
        "# tasks\n\n- [ ] 何かやる #work\n    - 2026-07-20: 着手\n- [ ] 別のこと #home\n    - 2026-07-20: 着手\n",
    );

    let out = run_list(root, &["--tag", "work", "--format", "json"]);
    assert!(out.success);
    let json: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let texts: Vec<String> = json
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["fileGroups"].as_array().unwrap())
        .flat_map(|f| f["tasks"].as_array().unwrap())
        .map(|t| t["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(texts, ["何かやる #work"]);
}

#[test]
fn test_unsupported_format_fails_even_with_no_match() {
    let home = TempHome::new("bad-format");
    let root = home.path();
    write_md(root, "tasks.md", "# tasks\n\n- [ ] 何かやる #work\n");

    // 0件でもフォーマットの誤りは黙って通さない
    let out = run_list(root, &["--tag", "zzz_notexist", "--format", "csv"]);
    assert!(!out.success, "未対応フォーマットは失敗すべき");
}
