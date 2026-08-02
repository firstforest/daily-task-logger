use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WikiLinkMatch {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedName {
    pub name: String,
    pub is_journal: bool,
}

pub fn normalize_wiki_name(raw: &str) -> NormalizedName {
    let trimmed = raw.trim();
    let without_ext = trimmed
        .strip_suffix(".md")
        .unwrap_or(trimmed)
        .to_string();

    let date_re = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
    let is_journal = date_re.is_match(&without_ext);

    NormalizedName {
        name: without_ext,
        is_journal,
    }
}

pub fn resolve_wiki_link(name: &str, candidates: &[PathBuf]) -> Option<PathBuf> {
    let target = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());

    for candidate in candidates {
        let stem = candidate
            .file_stem()
            .map(|s| s.to_string_lossy().to_string());
        if stem.as_deref() == Some(target.as_str()) {
            return Some(candidate.clone());
        }
    }
    None
}

pub fn wiki_link_create_path(name: &str, is_journal: bool, taski_home: &Path) -> PathBuf {
    if is_journal {
        let year = &name[0..4];
        let month = &name[5..7];
        taski_home
            .join("journal")
            .join(year)
            .join(month)
            .join(format!("{name}.md"))
    } else {
        taski_home.join("note").join(format!("{name}.md"))
    }
}

pub fn wiki_link_initial_content(name: &str) -> String {
    format!("# {name}\n")
}

/// 行単位で何度も呼ばれるのでコンパイル結果を使い回す（`journal_work` は
/// journal のタスク行ごとにこれを引く）。
fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\[\]|]+?)\]\]").unwrap())
}

pub fn parse_wiki_links(text: &str) -> Vec<WikiLinkMatch> {
    link_re()
        .captures_iter(text)
        .filter_map(|caps| {
            let whole = caps.get(0)?;
            let name = caps.get(1)?.as_str().to_string();
            if name.is_empty() {
                return None;
            }
            Some(WikiLinkMatch {
                name,
                start: whole.start(),
                end: whole.end(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_link() {
        let text = "ここに [[foo]] があります";
        let got = parse_wiki_links(text);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "foo");
    }

    #[test]
    fn test_parse_link_with_md_extension() {
        let got = parse_wiki_links("[[bar.md]]");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "bar.md");
    }

    #[test]
    fn test_parse_multiple_links() {
        let got = parse_wiki_links("[[a]] と [[b]] と [[c]]");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].name, "a");
        assert_eq!(got[1].name, "b");
        assert_eq!(got[2].name, "c");
    }

    #[test]
    fn test_parse_ignores_pipes_and_brackets() {
        assert_eq!(parse_wiki_links("[[foo|表示名]]").len(), 0);
        assert_eq!(parse_wiki_links("[[]]").len(), 0);
    }

    #[test]
    fn test_parse_returns_byte_offsets() {
        let text = "xx[[foo]]yy";
        let got = parse_wiki_links(text);
        assert_eq!(got[0].start, 2);
        assert_eq!(got[0].end, 9);
        assert_eq!(&text[got[0].start..got[0].end], "[[foo]]");
    }

    #[test]
    fn test_normalize_plain() {
        let got = normalize_wiki_name("foo");
        assert_eq!(got.name, "foo");
        assert!(!got.is_journal);
    }

    #[test]
    fn test_normalize_strips_md_extension() {
        let got = normalize_wiki_name("foo.md");
        assert_eq!(got.name, "foo");
        assert!(!got.is_journal);
    }

    #[test]
    fn test_normalize_detects_journal_date() {
        let got = normalize_wiki_name("2026-04-14");
        assert_eq!(got.name, "2026-04-14");
        assert!(got.is_journal);
    }

    #[test]
    fn test_normalize_trims_whitespace() {
        let got = normalize_wiki_name("  foo  ");
        assert_eq!(got.name, "foo");
    }

    #[test]
    fn test_resolve_finds_first_match() {
        let candidates = vec![
            PathBuf::from("/home/u/taski/foo.md"),
            PathBuf::from("/home/u/work/foo.md"),
        ];
        let got = resolve_wiki_link("foo", &candidates);
        assert_eq!(got, Some(PathBuf::from("/home/u/taski/foo.md")));
    }

    #[test]
    fn test_resolve_matches_stem_ignoring_extension() {
        let candidates = vec![PathBuf::from("/a/foo.md")];
        assert_eq!(
            resolve_wiki_link("foo", &candidates),
            Some(PathBuf::from("/a/foo.md"))
        );
    }

    #[test]
    fn test_resolve_returns_none_when_absent() {
        let candidates = vec![PathBuf::from("/a/bar.md")];
        assert_eq!(resolve_wiki_link("foo", &candidates), None);
    }

    #[test]
    fn test_resolve_matches_journal_date() {
        let candidates = vec![PathBuf::from(
            "/home/u/taski/journal/2026/04/2026-04-14.md",
        )];
        assert_eq!(
            resolve_wiki_link("2026-04-14", &candidates),
            Some(PathBuf::from(
                "/home/u/taski/journal/2026/04/2026-04-14.md"
            ))
        );
    }

    #[test]
    fn test_create_path_note() {
        let home = PathBuf::from("/home/u/taski");
        let got = wiki_link_create_path("foo", false, &home);
        assert_eq!(got, PathBuf::from("/home/u/taski/note/foo.md"));
    }

    #[test]
    fn test_create_path_journal() {
        let home = PathBuf::from("/home/u/taski");
        let got = wiki_link_create_path("2026-04-14", true, &home);
        assert_eq!(
            got,
            PathBuf::from("/home/u/taski/journal/2026/04/2026-04-14.md")
        );
    }

    #[test]
    fn test_initial_content() {
        assert_eq!(wiki_link_initial_content("foo"), "# foo\n");
    }
}
