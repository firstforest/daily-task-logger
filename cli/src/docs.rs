//! Document 集合を作る（docs/domain.md §1）。
//!
//! **走査の規則はここ 1 本だけにする**（docs/design.md G-6）。以前は `taski list` が
//! taski home 全体を再帰、ジャーナルが `journal/` を再帰、PJ ノートが `note/` 直下のみ、
//! という 3 つの走査が別々に書かれていた。非対称なのは意図ではなく実装の都合で、
//! 実際 `note/sub/名前.md` は Wiki リンクとしては開けるのに PJ としては拾われない、
//! という食い違いを生んでいた。
//!
//! 用途ごとの絞り込みは走査規則ではなく**述語**で表す。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

/// ディレクトリ配下の `.md` を再帰的に集める。パス順にソートして返す。
///
/// 読めないディレクトリは黙って飛ばす。走査対象が存在しないのは普通のこと
/// （`note/` を作っていない、`journal/` がまだ無い）なので、エラーにしない。
pub fn collect_md_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(dir, &mut files);
    files.sort();
    files
}

fn collect_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
}

/// PJ ノートの候補（`note/**/*.md`）。
///
/// front matter に `project:` を持つかどうかは見ない。それは層 4（Document → Project の
/// 役割）であって走査の話ではないので、呼び出し側が判定する（domain.md §3）。
pub fn note_files(base_dir: &Path) -> Vec<PathBuf> {
    collect_md_files(&base_dir.join("note"))
}

/// ジャーナル（`journal/**/<YYYY-MM-DD>.md`）を日付の降順で返す。
///
/// ファイル名が日付でないものは落とす。`journal/` に置いた覚書などを 1 日分の
/// 記録として読まないため（syntax.md §2.1）。
pub fn journal_files_desc(base_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = collect_md_files(&base_dir.join("journal"))
        .into_iter()
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().to_string();
            NaiveDate::parse_from_str(&stem, "%Y-%m-%d")
                .ok()
                .map(|_| (stem, path))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "taski-docs-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn test_collect_is_recursive_and_sorted() {
        let root = temp_dir("recursive");
        write(&root, "b.md");
        write(&root, "sub/a.md");
        write(&root, "sub/deep/c.md");
        write(&root, "skip.txt");

        let got: Vec<String> = collect_md_files(&root)
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(got, vec!["b.md", "sub/a.md", "sub/deep/c.md"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_collect_missing_directory_is_empty() {
        let root = temp_dir("missing");
        assert!(collect_md_files(&root.join("なし")).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_note_files_include_subdirectories() {
        // `note/` 直下だけを見ていた頃は拾えなかった（G-6）
        let root = temp_dir("notes");
        write(&root, "note/直下.md");
        write(&root, "note/sub/配下.md");

        let mut got: Vec<String> = note_files(&root)
            .iter()
            .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
            .collect();
        got.sort();
        assert_eq!(got, vec!["直下", "配下"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_journal_files_are_dated_and_descending() {
        let root = temp_dir("journals");
        write(&root, "journal/2026/07/2026-07-20.md");
        write(&root, "journal/2026/08/2026-08-02.md");
        write(&root, "journal/覚書.md");

        let got: Vec<String> = journal_files_desc(&root)
            .into_iter()
            .map(|(date, _)| date)
            .collect();
        assert_eq!(got, vec!["2026-08-02", "2026-07-20"]);
        let _ = fs::remove_dir_all(&root);
    }
}
