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

/// パスのファイル名から拡張子を落としたもの（docs/domain.md §4 の `stem`）。
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 照合キー（docs/domain.md §4 の `match_key`）。空白を `_` に置換する。
///
/// `[[名前]]` には空白を書けるが `#タグ` には書けないので、突き合わせるには空白を
/// 潰した形を経由するしかない（docs/syntax.md §6）。したがって参照側と PJ 名側の
/// **両方**にこれを掛けてから比較する。片側だけに掛けると `在庫_管理.md` という
/// ノートが `[[在庫 管理]]` で引けない。
///
/// この変換は非可逆で `在庫 管理` と `在庫_管理` は同じキーになる。曖昧さは
/// 「黙って片方を採る」のではなく、探索範囲の中でキーが一意であることを要求して
/// 表面化させる（[`crate::pj::PjId`]）。
pub fn match_key(name: &str) -> String {
    name.replace(' ', "_")
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

/// 参照 `r` が文書 `d` を指すか（docs/domain.md §4 の `hits`）。
///
/// 名前レベルの原子であり、参照の解決はすべてこの述語の上に載る。照合キーを
/// **両側**に掛けて比べるので、`[[在庫 管理]]` と `#在庫_管理` は同じ文書を指す。
///
/// PJ の照合（`cli::pj`）も同じ関係を使う。かつては「開くときはファイル名の完全一致、
/// 集計するときは文字列一致」と 2 系統に分かれており、`note/在庫_管理.md` が
/// `[[在庫 管理]]` で開けないといった食い違いがあった（design.md G-5）。
pub fn hits(path: &Path, ref_text: &str) -> bool {
    match_key(&stem(path)) == match_key(ref_text)
}

/// 参照を解決する（docs/domain.md §4 の `resolve`）。
///
/// **候補列の順序が優先順位を表す。** requirements.md 3.4 が定める
/// 「`$HOME/taski` > ワークスペース > 追加ディレクトリ > 開いているドキュメント」は、
/// 呼び出し側が候補をこの順に並べることで表現する。したがって複数一致したときは
/// 先頭を採る。
///
/// domain.md §4 は「唯一の `d`（無ければ `None`）」と定めているが、上の優先順位が
/// 要求である以上、探索範囲をまたいだ一致を曖昧として捨てるわけにはいかない。
/// 一意性を課せるのは 1 つの探索範囲の中だけで、実際に課しているのは PJ ノートの
/// 集合に対してだけである（`cli::pj` が衝突を警告する。design.md W-9）。
pub fn resolve(ref_text: &str, candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|path| hits(path, ref_text))
        .cloned()
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
    fn test_stem_drops_extension() {
        assert_eq!(stem(Path::new("/a/b/在庫 管理.md")), "在庫 管理");
        assert_eq!(stem(Path::new("plain")), "plain");
    }

    #[test]
    fn test_match_key_replaces_spaces() {
        assert_eq!(match_key("在庫 管理"), "在庫_管理");
    }

    #[test]
    fn test_match_key_is_idempotent() {
        // 参照側と PJ 名側の両方に掛けるので、既に `_` の形に掛け直しても壊れないこと
        assert_eq!(match_key("在庫_管理"), "在庫_管理");
        assert_eq!(match_key(&match_key("在庫 管理")), match_key("在庫 管理"));
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
        let got = resolve("foo", &candidates);
        assert_eq!(got, Some(PathBuf::from("/home/u/taski/foo.md")));
    }

    #[test]
    fn test_resolve_matches_stem_ignoring_extension() {
        let candidates = vec![PathBuf::from("/a/foo.md")];
        assert_eq!(
            resolve("foo", &candidates),
            Some(PathBuf::from("/a/foo.md"))
        );
    }

    #[test]
    fn test_resolve_returns_none_when_absent() {
        let candidates = vec![PathBuf::from("/a/bar.md")];
        assert_eq!(resolve("foo", &candidates), None);
    }

    #[test]
    fn test_resolve_matches_journal_date() {
        let candidates = vec![PathBuf::from(
            "/home/u/taski/journal/2026/04/2026-04-14.md",
        )];
        assert_eq!(
            resolve("2026-04-14", &candidates),
            Some(PathBuf::from(
                "/home/u/taski/journal/2026/04/2026-04-14.md"
            ))
        );
    }

    #[test]
    fn test_resolve_goes_through_the_match_key() {
        // `#タグ` に空白を書けない以上、ノート名に `_` を使うことがある。
        // 開くときも集計と同じ照合キーを通す（G-5）
        let candidates = vec![PathBuf::from("/a/在庫_管理.md")];
        assert_eq!(
            resolve("在庫 管理", &candidates),
            Some(PathBuf::from("/a/在庫_管理.md"))
        );
        assert_eq!(
            resolve("在庫_管理", &candidates),
            Some(PathBuf::from("/a/在庫_管理.md"))
        );
    }

    #[test]
    fn test_resolve_keeps_dots_in_the_name() {
        // 参照名を `Path::file_stem` に通していたころは `v1.2 設計` が `v1` に潰れていた
        let candidates = vec![PathBuf::from("/a/v1.2 設計.md")];
        assert_eq!(
            resolve("v1.2 設計", &candidates),
            Some(PathBuf::from("/a/v1.2 設計.md"))
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
