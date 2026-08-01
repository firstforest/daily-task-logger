//! CLI 統合テストの共通ヘルパ。
//!
//! 統合テストはファイルごとに別クレートとしてコンパイルされるので、
//! `tests/<サブコマンド>_cli.rs` それぞれから `mod common;` で取り込む。
//! サブコマンドごとに 1 ファイルというルール上、ここに置かないと
//! ファイルが増えるたびに同じ `TempHome` が複製されていく。
//!
//! 各テストクレートは必要なヘルパだけを使うので、使われないものが必ず出る。

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// テスト用の一時ディレクトリ。`$HOME` に見立てて使い、Drop で消す。
///
/// 名前の一意性は pid と nanos が担保する。`label` は残骸を追うときの手掛かり。
pub struct TempHome(PathBuf);

impl TempHome {
    pub fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("taski-test-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        TempHome(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `$HOME/taski/<name>` に Markdown を書く。中間ディレクトリは作る。
pub fn write_md(home: &Path, name: &str, body: &str) {
    let path = home.join("taski").join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// `$HOME/taski` だけ作ってファイルは置かない。
pub fn make_empty_taski(home: &Path) {
    fs::create_dir_all(home.join("taski")).unwrap();
}
