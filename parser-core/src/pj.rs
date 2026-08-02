//! PJ ノート（front matter に `project:` を持つ `note/*.md`）の解析。
//!
//! PJ ノートは日報形式で、次の3セクションを持つ。
//!
//! ```markdown
//! ## 次の予定
//! - [ ] Web公開の準備（30分・重・@PC）
//!
//! ## ログ
//! - 2026-07-30: base path を追加した
//!
//! ## オープンタスク
//! - オートセーブを実装する
//! ```
//!
//! 記法の使い分けが設計の核心で、`## 次の予定` だけがチェックボックスを持つ。
//! これにより `taski list`（日付軸）に流れ込む PJ タスクが active PJ あたり最大1つに絞られる。

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

/// 「次の予定」セクションの見出し名。
pub const SECTION_NEXT: &str = "次の予定";
/// 「ログ」セクションの見出し名。
pub const SECTION_LOG: &str = "ログ";
/// 「オープンタスク」セクションの見出し名。
pub const SECTION_BACKLOG: &str = "オープンタスク";

/// PJ の正規名 = ノートのファイル名（docs/domain.md §4）。
///
/// 参照との照合は正規形どうしの比較ではなく [`match_key`] を経由する。名前をそのまま
/// `String` で持ち回ると、照合キーを掛け忘れた比較（`refs.contains(name)`）と
/// 掛けた比較が混在し、`[[在庫 管理]]` と `#在庫_管理` のどちらか片方しか当たらない
/// という取りこぼしが起きる。掛ける場所を [`Self::match_key`] 1 箇所に閉じる。
///
/// JSON の表現は素の文字列のまま（`#[serde(transparent)]`。docs/design.md §11）。
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(transparent)]
pub struct PjId(String);

impl PjId {
    pub fn new(name: impl Into<String>) -> Self {
        PjId(name.into())
    }

    /// ノートのパスから作る（`stem(path)`）。
    pub fn from_path(path: &std::path::Path) -> Self {
        PjId(crate::wiki_link::stem(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 参照と突き合わせるためのキー。
    pub fn match_key(&self) -> String {
        crate::wiki_link::match_key(&self.0)
    }
}

impl std::fmt::Display for PjId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// PJ の健全性。
/// - `NoNext`: `## 次の予定` に `- [ ]` が無い（空 / セクション欠落 / `- [x]`・`- [-]` のみ）
/// - `Unclarified`: 次の予定はあるが判断メタデータが無い
/// - `Ok`: 次の予定があり判断メタデータも付いている
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PjHealth {
    Ok,
    Unclarified,
    NoNext,
}

/// `## ログ` のエントリ（`- YYYY-MM-DD: 内容`）。
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PjLogEntry {
    pub date: String,
    pub text: String,
}

/// PJ ノート本文から取り出せる情報。git やファイルシステムに依存しない部分だけを持つ。
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PjNote {
    /// `## 次の予定` の最初の `- [ ]` 行（メタデータ込みの原文）
    pub next_action: Option<String>,
    /// 判断メタデータを取り除いた本文
    pub next_action_body: Option<String>,
    /// 判断メタデータ（`30分・重・@PC`）。無ければ `None`
    pub next_action_meta: Option<String>,
    /// 次の予定が `@AI` か
    pub next_action_ai: bool,
    pub health: PjHealth,
    /// `## ログ` のエントリ。新しい順
    pub logs: Vec<PjLogEntry>,
    /// `## オープンタスク` の項目（チェックボックスなしの `- ` 行）
    pub backlog: Vec<String>,
}

impl PjNote {
    /// 最新のログ日付。
    pub fn log_last(&self) -> Option<&str> {
        self.logs.first().map(|l| l.date.as_str())
    }
}

// 正規表現は行ごと・メタデータ要素ごとに引かれるので、初回だけコンパイルして使い回す。
//
// 行の構造を区切る空白は `\s` ではなく `[ \t]` で書く。`\s` は Unicode 空白（全角
// スペース U+3000 など）にも一致するので、`　- [ ] 本文` がタスクになってしまう。
// 記法上の空白は半角スペースとタブだけ（docs/syntax.md §3）。

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(#{1,6})[ \t]+(.*)").unwrap())
}

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*(```|~~~)").unwrap())
}

/// タスク行（`- [ ]` / `- [x]` / `- [-]`）。`- [[ノート名]]` は `[` の次が `[` なので一致しない。
fn task_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*-[ \t]*\[([ x-])\][ \t]*(.*)").unwrap())
}

/// ログ行（`- YYYY-MM-DD: 内容`）。時刻・時間範囲付きも許容する。
fn log_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[ \t]*-[ \t]*(\d{4}-\d{2}-\d{2})(?:[ \t]+\d{1,2}:\d{2}(?:-\d{1,2}:\d{2})?)?:[ \t]*(.*)",
        )
        .unwrap()
    })
}

/// 時刻付きログ行（`- YYYY-MM-DD HH:MM: 内容` / `- YYYY-MM-DD HH:MM-HH:MM: 内容`）。
/// 時刻の無いログは一致しない。インデントを取るので `log_re` とは別に引く。
fn timed_log_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([ \t]*)-[ \t]*(\d{4}-\d{2}-\d{2})[ \t]+\d{1,2}:\d{2}(?:-\d{1,2}:\d{2})?:")
            .unwrap()
    })
}

/// インデント付きのタスク行。`task_re` と同じ判定だが先頭の空白を取る。
fn indented_task_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([ \t]*)-[ \t]*\[([ x-])\][ \t]*(.*)").unwrap())
}

/// 箇条書き行（`- 内容`）。
fn bullet_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*-[ \t]+(.*)").unwrap())
}

/// 行末の括弧（`（45分・重・@PC）`）。
fn trailing_paren_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.*)[（(]([^（）()]{1,60})[）)]\s*$").unwrap())
}

/// 所要時間（`45分` / `2時間`）。
fn duration_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+\s*(分|時間)$").unwrap())
}

/// 締切（`08-15` / `締切08-15`）。
fn deadline_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(締切)?\d{1,2}-\d{1,2}$").unwrap())
}

/// `##` 見出しでセクションに切り分ける。
///
/// 各セクションの範囲は次の `#` または `##` 見出しまで。`###` 以下はセクション内に含める
/// （`## オープンタスク` の中で `###` によるグループ分けをしてよい、という規約のため）。
/// コードフェンス内の見出しは無視する。
fn split_sections(lines: &[String]) -> HashMap<String, Vec<String>> {
    let heading = heading_re();
    let fence = fence_re();

    let mut sections: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<String> = None;
    let mut in_code_block = false;

    for line in lines {
        if fence.is_match(line) {
            in_code_block = !in_code_block;
            if let Some(ref name) = current {
                sections.entry(name.clone()).or_default().push(line.clone());
            }
            continue;
        }
        if !in_code_block {
            if let Some(caps) = heading.captures(line) {
                if caps[1].len() <= 2 {
                    current = Some(caps[2].trim().to_string());
                    sections.entry(caps[2].trim().to_string()).or_default();
                    continue;
                }
            }
        }
        if let Some(ref name) = current {
            sections.entry(name.clone()).or_default().push(line.clone());
        }
    }

    sections
}

/// セクション内の行をコードフェンスを除いて走査する。
fn without_code_blocks(lines: &[String]) -> Vec<&String> {
    let fence = fence_re();
    let mut in_code_block = false;
    let mut result = Vec::new();
    for line in lines {
        if fence.is_match(line) {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block {
            result.push(line);
        }
    }
    result
}

/// 判断メタデータの1要素として妥当か判定する。
///
/// - 所要時間: `45分` / `2時間`
/// - 重さ: `軽` / `重`
/// - コンテキスト: `@PC` / `@AI` / `@家` など
/// - 締切: `08-15` / `締切08-15`
fn is_meta_element(element: &str) -> bool {
    let e = element.trim();
    // 重さ（`軽` / `重`）とコンテキスト（`@...`）は正規表現を使うまでもない
    let weight = e == "軽" || e == "重";
    let context = e.starts_with('@') && e.chars().count() > 1;
    weight || context || duration_re().is_match(e) || deadline_re().is_match(e)
}

/// 行末の `（45分・重・@PC）` を判断メタデータとして本文から分離する。
///
/// メタデータでなければ `None`。単なる注記（`（仮）`）や列挙（`（髪・服・顔）`）を
/// 誤検出しないよう、括弧の中身を `・,、` で分割していずれかの要素が
/// メタデータの書式に一致することを条件にする。
pub fn split_decision_meta(text: &str) -> Option<(String, String)> {
    let caps = trailing_paren_re().captures(text)?;
    let body = caps[1].trim_end().to_string();
    let meta = caps[2].to_string();

    let matched = meta
        .split(['・', ',', '、', '，'])
        .any(is_meta_element);
    if !matched {
        return None;
    }
    Some((body, meta))
}

/// `## 次の予定` セクションから最初の未完了タスク（`- [ ]`）を取り出す。
///
/// `- [x]` / `- [-]` しか無い場合は「完了したが次を決めていない」状態なので `None` を返す。
fn extract_next_action(section: Option<&Vec<String>>) -> Option<String> {
    let section = section?;
    let task = task_re();
    for line in without_code_blocks(section) {
        if let Some(caps) = task.captures(line) {
            if &caps[1] == " " {
                let text = caps[2].trim();
                if text.is_empty() {
                    continue;
                }
                return Some(text.to_string());
            }
        }
    }
    None
}

/// `## ログ` セクションからログエントリを取り出す。新しい順に並べ替える。
fn extract_logs(section: Option<&Vec<String>>) -> Vec<PjLogEntry> {
    let Some(section) = section else {
        return Vec::new();
    };
    let re = log_re();
    let mut logs: Vec<PjLogEntry> = without_code_blocks(section)
        .into_iter()
        .filter_map(|line| {
            re.captures(line).map(|caps| PjLogEntry {
                date: caps[1].to_string(),
                text: caps[2].trim().to_string(),
            })
        })
        .collect();
    // 日付の降順（同日は記載順を保つ安定ソート）
    logs.sort_by(|a, b| b.date.cmp(&a.date));
    logs
}

/// `## オープンタスク` セクションからバックログ項目を取り出す。
///
/// チェックボックス付きの行とログ行は含めない（バックログはチェックボックスなしの `- ` 行）。
fn extract_backlog(section: Option<&Vec<String>>) -> Vec<String> {
    let Some(section) = section else {
        return Vec::new();
    };
    let task = task_re();
    let log = log_re();
    let bullet = bullet_re();
    without_code_blocks(section)
        .into_iter()
        .filter(|line| !task.is_match(line) && !log.is_match(line))
        .filter_map(|line| bullet.captures(line).map(|caps| caps[1].trim().to_string()))
        .filter(|text| !text.is_empty())
        .collect()
}

/// PJ ノート本文を解析する。
pub fn parse_pj_note(lines: &[String]) -> PjNote {
    let sections = split_sections(lines);

    let next_action = extract_next_action(sections.get(SECTION_NEXT));
    let (next_action_body, next_action_meta) = match next_action {
        Some(ref text) => match split_decision_meta(text) {
            Some((body, meta)) => (Some(body), Some(meta)),
            None => (Some(text.clone()), None),
        },
        None => (None, None),
    };

    let health = match (&next_action, &next_action_meta) {
        (None, _) => PjHealth::NoNext,
        (Some(_), None) => PjHealth::Unclarified,
        (Some(_), Some(_)) => PjHealth::Ok,
    };

    let next_action_ai = next_action_meta
        .as_deref()
        .map(|meta| meta.split(['・', ',', '、', '，']).any(|e| e.trim() == "@AI"))
        .unwrap_or(false);

    PjNote {
        next_action,
        next_action_body,
        next_action_meta,
        next_action_ai,
        health,
        logs: extract_logs(sections.get(SECTION_LOG)),
        backlog: extract_backlog(sections.get(SECTION_BACKLOG)),
    }
}

// === journal の実働 ===

/// journal から拾った「実働」1件。
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct JournalWork {
    /// 実働と判定した日付
    pub date: String,
    /// そのタスク行が参照している名前（`[[名前]]` の中身と `#タグ`）
    pub refs: Vec<String>,
}

/// テキストが参照している名前を集める。`[[名前]]`（`.md` を落とした正規化名）と `#タグ`。
///
/// 言及（journal 全文）と実働（タスク行）で同じ関数を使うために公開する。片方だけが
/// `[[名前.md]]` のような表記を拾うと、実働はあるのに言及が `null` という
/// 成り立たない組み合わせが出る（実働は言及の部分集合でなければならない）。
pub fn collect_refs(text: &str) -> Vec<String> {
    let mut refs: Vec<String> = crate::wiki_link::parse_wiki_links(text)
        .into_iter()
        .map(|m| crate::wiki_link::normalize_wiki_name(&m.name).name)
        .collect();
    refs.extend(crate::extract_tags(text));
    refs
}

/// 文書全体から参照を集める。コードフェンスの中は数えない。
///
/// 「言及」の判定に使う。ファイル全文へ一気に `collect_refs` を掛けるとフェンス内も
/// 拾ってしまい、記法の例としてコードブロックに書いた `[[名前]]` で `journal_last` が
/// 更新される。フェンス内は全解析から除外する（docs/syntax.md §2.3）。
///
/// 実働側（`journal_work`）もフェンスを飛ばしたうえでタスク行の本文に `collect_refs`
/// を掛けるので、タスク行はここで拾う行の部分集合になり「実働 ⊆ 言及」が保たれる。
pub fn collect_document_refs(lines: &[String]) -> Vec<String> {
    let fence = fence_re();
    let mut refs: Vec<String> = Vec::new();
    let mut in_code_block = false;

    for line in lines {
        if fence.is_match(line) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        refs.extend(collect_refs(line));
    }

    refs
}

/// 行頭の空白幅。タスク行・ログ行の `^([ \t]*)` キャプチャと同じ数え方に揃える。
///
/// `trim_start` は Unicode 空白まで落とすので使えない。全角スペースで字下げした行は
/// 正規表現側ではインデント 0 になるため、ここで 3（UTF-8 のバイト数）を返すと
/// 帰属の判定が食い違う。
fn indent_width(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// journal 1日分の本文から「実働」を取り出す。
///
/// `## 今日の候補` に載っただけ・他タスクの文中で触れられただけの「言及」と区別するため、
/// 参照を持つタスク行そのものが次のいずれかを満たす場合だけ実働と見なす。
///
/// - `- [x]` で完了している → その journal の日付（`file_date`）
/// - 時刻付きログ（`- YYYY-MM-DD HH:MM: ...`）を持つ → そのログ行の日付
///
/// 時刻の無いログ（`- YYYY-MM-DD: ...`）は実働と見なさない。journal では
/// 「やった記録」ではなく予定・メモとしても書かれるため。
///
/// 時刻付きログは**そのタスクの配下**（より深いインデント）にあるものだけを見る。
/// 見出し・兄弟の箇条書き・段落が挟まった時点でタスクの文脈を閉じるので、
/// `## 今日の候補` に PJ を並べた後、別セクションに無関係な時刻メモを書いても
/// 実働にはならない。ここを緩めると「候補に載せただけ」が実働に化ける。
pub fn journal_work(lines: &[String], file_date: &str) -> Vec<JournalWork> {
    let task = indented_task_re();
    let timed = timed_log_re();
    let fence = fence_re();

    let mut result: Vec<JournalWork> = Vec::new();
    // 参照を持つタスク行の (インデント, 参照名)。配下でない行が来たら閉じる
    let mut current: Option<(usize, Vec<String>)> = None;
    let mut in_code_block = false;

    for line in lines {
        if fence.is_match(line) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        if let Some(caps) = task.captures(line) {
            let refs = collect_refs(&caps[3]);
            if refs.is_empty() {
                current = None;
                continue;
            }
            if &caps[2] == "x" {
                result.push(JournalWork {
                    date: file_date.to_string(),
                    refs: refs.clone(),
                });
            }
            current = Some((caps[1].len(), refs));
            continue;
        }

        let Some((indent, refs)) = current.as_ref() else {
            continue;
        };
        // ログ行はタスク行より深いインデントであることを必須にする
        if let Some(caps) = timed.captures(line) {
            if caps[1].len() > *indent {
                result.push(JournalWork {
                    date: caps[2].to_string(),
                    refs: refs.clone(),
                });
                continue;
            }
        }
        // タスクの配下でない行（見出し・兄弟の箇条書き・段落）が来たら文脈を閉じる。
        // 空行だけでは閉じない。ログの間に空行を挟む書き方を落とさないため
        if !line.trim().is_empty() && indent_width(line) <= *indent {
            current = None;
        }
    }

    result
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    // --- split_decision_meta ---

    #[test]
    fn test_meta_full() {
        let (body, meta) = split_decision_meta("カード作成（60分・重・@PC）").unwrap();
        assert_eq!(body, "カード作成");
        assert_eq!(meta, "60分・重・@PC");
    }

    #[test]
    fn test_meta_without_context() {
        let (body, meta) = split_decision_meta("画像生成の準備（30分・軽）").unwrap();
        assert_eq!(body, "画像生成の準備");
        assert_eq!(meta, "30分・軽");
    }

    #[test]
    fn test_meta_with_deadline() {
        let (body, meta) =
            split_decision_meta("納品する（30分・軽・@PC・締切08-15）").unwrap();
        assert_eq!(body, "納品する");
        assert_eq!(meta, "30分・軽・@PC・締切08-15");
    }

    #[test]
    fn test_meta_plain_sentence_is_not_meta() {
        assert_eq!(split_decision_meta("会計の仕事をやる"), None);
    }

    #[test]
    fn test_meta_enumeration_is_not_meta() {
        // 「・」を含むだけでは通さない（プロトタイプで踏んだ偽陽性）
        assert_eq!(split_decision_meta("テクスチャペイント（髪・服・顔）"), None);
    }

    #[test]
    fn test_meta_note_is_not_meta() {
        assert_eq!(split_decision_meta("対応する（仮）"), None);
    }

    #[test]
    fn test_meta_only_trailing_paren_used() {
        let (body, meta) =
            split_decision_meta("カードデザインの作成（マナ?と選択式）（60分・重・@PC）").unwrap();
        assert_eq!(body, "カードデザインの作成（マナ?と選択式）");
        assert_eq!(meta, "60分・重・@PC");
    }

    #[test]
    fn test_meta_ascii_paren() {
        let (body, meta) = split_decision_meta("do it (30分・軽)").unwrap();
        assert_eq!(body, "do it");
        assert_eq!(meta, "30分・軽");
    }

    #[test]
    fn test_meta_hours() {
        assert!(split_decision_meta("大掃除（2時間・重）").is_some());
    }

    #[test]
    fn test_meta_no_paren() {
        assert_eq!(split_decision_meta("メタデータなし"), None);
    }

    // --- parse_pj_note: next action / health ---

    #[test]
    fn test_pj_note_ok() {
        let l = lines(&[
            "---",
            "project: active",
            "---",
            "# 漫画制作エディタ",
            "",
            "## 次の予定",
            "- [ ] Web公開の準備（ビルド設定の確認）（30分・重・@PC）",
            "",
            "## ログ",
            "- 2026-07-30: base path を追加した",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::Ok);
        assert_eq!(
            pj.next_action.as_deref(),
            Some("Web公開の準備（ビルド設定の確認）（30分・重・@PC）")
        );
        assert_eq!(
            pj.next_action_body.as_deref(),
            Some("Web公開の準備（ビルド設定の確認）")
        );
        assert_eq!(pj.next_action_meta.as_deref(), Some("30分・重・@PC"));
        assert!(!pj.next_action_ai);
    }

    #[test]
    fn test_pj_note_unclarified() {
        let l = lines(&["## 次の予定", "- [ ] 会計の仕事をやる"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::Unclarified);
        assert_eq!(pj.next_action_body.as_deref(), Some("会計の仕事をやる"));
        assert_eq!(pj.next_action_meta, None);
    }

    #[test]
    fn test_pj_note_no_next_when_section_missing() {
        let l = lines(&["# PJ", "本文だけ"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::NoNext);
        assert_eq!(pj.next_action, None);
    }

    #[test]
    fn test_pj_note_no_next_when_section_empty() {
        let l = lines(&["## 次の予定", "", "## ログ", "- 2026-07-30: なにか"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::NoNext);
    }

    #[test]
    fn test_pj_note_no_next_when_only_completed() {
        // 完了したが次を決めていない状態。真っ先に手を入れるべき PJ
        let l = lines(&["## 次の予定", "- [x] 終わったタスク"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::NoNext);
    }

    #[test]
    fn test_pj_note_no_next_when_only_cancelled() {
        let l = lines(&["## 次の予定", "- [-] 見送ったタスク"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::NoNext);
    }

    #[test]
    fn test_pj_note_first_incomplete_wins() {
        let l = lines(&[
            "## 次の予定",
            "- [x] 終わったタスク",
            "- [ ] 本命（30分・軽）",
            "- [ ] さらに次",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.next_action_body.as_deref(), Some("本命"));
    }

    #[test]
    fn test_pj_note_next_action_ai() {
        let l = lines(&["## 次の予定", "- [ ] 調査する（30分・軽・@AI）"]);
        let pj = parse_pj_note(&l);
        assert!(pj.next_action_ai);
    }

    #[test]
    fn test_pj_note_wiki_link_bullet_is_not_task() {
        // `- [[ノート名]]` をチェックボックスと誤判定しない
        let l = lines(&["## 次の予定", "- [[別のノート]] を参照"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::NoNext);
        assert_eq!(pj.next_action, None);
    }

    // --- parse_pj_note: logs ---

    #[test]
    fn test_pj_note_logs_newest_first() {
        let l = lines(&[
            "## ログ",
            "- 2026-07-15: 古いログ",
            "- 2026-07-30: 新しいログ",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.logs.len(), 2);
        assert_eq!(pj.logs[0].date, "2026-07-30");
        assert_eq!(pj.logs[0].text, "新しいログ");
        assert_eq!(pj.logs[1].date, "2026-07-15");
        assert_eq!(pj.log_last(), Some("2026-07-30"));
    }

    #[test]
    fn test_pj_note_log_with_time() {
        let l = lines(&["## ログ", "- 2026-07-30 10:00: 時刻つき"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.logs.len(), 1);
        assert_eq!(pj.logs[0].date, "2026-07-30");
        assert_eq!(pj.logs[0].text, "時刻つき");
    }

    #[test]
    fn test_pj_note_logs_empty() {
        let l = lines(&["## 次の予定", "- [ ] なにか"]);
        let pj = parse_pj_note(&l);
        assert!(pj.logs.is_empty());
        assert_eq!(pj.log_last(), None);
    }

    // --- parse_pj_note: backlog ---

    #[test]
    fn test_pj_note_backlog() {
        let l = lines(&[
            "## オープンタスク",
            "- オートセーブを実装する",
            "- 改良を進める",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.backlog, vec!["オートセーブを実装する", "改良を進める"]);
    }

    #[test]
    fn test_pj_note_backlog_with_subheadings() {
        // `###` によるグループ分けはセクション内に含める
        let l = lines(&[
            "## オープンタスク",
            "",
            "### 前提",
            "- 移行する",
            "",
            "### 実装",
            "- 実装する",
            "",
            "## 未決事項",
            "- これは含めない",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.backlog, vec!["移行する", "実装する"]);
    }

    #[test]
    fn test_pj_note_backlog_excludes_checkbox_and_log() {
        let l = lines(&[
            "## オープンタスク",
            "- [ ] チェックボックスは除く",
            "- 2026-07-30: ログも除く",
            "- 通常のバックログ",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.backlog, vec!["通常のバックログ"]);
    }

    #[test]
    fn test_pj_note_backlog_includes_wiki_link_bullet() {
        let l = lines(&["## オープンタスク", "- [[関連ノート]] を整理する"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.backlog, vec!["[[関連ノート]] を整理する"]);
    }

    // --- section boundaries ---

    #[test]
    fn test_section_stops_at_next_h2() {
        let l = lines(&[
            "## 次の予定",
            "- [ ] 本命（30分・軽）",
            "## オープンタスク",
            "- [ ] ここは次の予定ではない",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.next_action_body.as_deref(), Some("本命"));
    }

    #[test]
    fn test_section_stops_at_h1() {
        let l = lines(&["## 次の予定", "# 別の見出し", "- [ ] 拾わない"]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.health, PjHealth::NoNext);
    }

    #[test]
    fn test_code_block_contents_ignored() {
        let l = lines(&[
            "## 次の予定",
            "```markdown",
            "- [ ] 記法サンプル（30分・軽）",
            "```",
            "- [ ] 実際の次の予定（60分・重・@PC）",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.next_action_body.as_deref(), Some("実際の次の予定"));
    }

    #[test]
    fn test_heading_inside_code_block_ignored() {
        let l = lines(&[
            "## 次の予定",
            "```markdown",
            "## ログ",
            "```",
            "- [ ] 本命（30分・軽）",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.next_action_body.as_deref(), Some("本命"));
        assert!(pj.logs.is_empty());
    }

    #[test]
    fn test_duplicate_sections_merged() {
        let l = lines(&[
            "## ログ",
            "- 2026-07-30: 一つ目",
            "## その他",
            "## ログ",
            "- 2026-07-31: 二つ目",
        ]);
        let pj = parse_pj_note(&l);
        assert_eq!(pj.logs.len(), 2);
        assert_eq!(pj.log_last(), Some("2026-07-31"));
    }

    // --- journal_work ---

    /// 日付の重複を潰して、参照名ごとの実働日を取り出すテスト用ヘルパ。
    fn work_dates(works: &[JournalWork], name: &str) -> Vec<String> {
        let mut dates: Vec<String> = works
            .iter()
            .filter(|w| w.refs.iter().any(|r| r == name))
            .map(|w| w.date.clone())
            .collect();
        dates.sort();
        dates.dedup();
        dates
    }

    #[test]
    fn test_journal_work_completed_task() {
        let l = lines(&["# 2026-08-01", "- [x] [[永夜]] のカードを作る"]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-08-01"]);
    }

    #[test]
    fn test_journal_work_incomplete_with_timed_log() {
        // 完了していなくても時刻付きログがあれば実働
        let l = lines(&[
            "# 2026-08-01",
            "- [ ] [[永夜]] のカードを作る",
            "    - 2026-08-01 10:00-11:30: 下書きまで",
        ]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-08-01"]);
    }

    #[test]
    fn test_journal_work_mention_only_is_not_work() {
        // 「今日の候補」に載っただけ・文中で触れただけは実働ではない（誤検出の元）
        let l = lines(&[
            "# 2026-08-01",
            "## 今日の候補",
            "- [ ] [[在庫管理]] を進める",
            "- [ ] 別のタスク（[[在庫管理]] とも関係する）",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_untimed_log_is_not_work() {
        // 時刻の無いログは予定・メモとしても書かれるので実働と見なさない
        let l = lines(&[
            "# 2026-08-01",
            "- [ ] [[在庫管理]] を進める",
            "    - 2026-08-01: あとでやる",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_cancelled_task_is_not_work() {
        // 見送り（着手せず）は実働ではない
        let l = lines(&["# 2026-08-01", "- [-] [[在庫管理]] を進める"]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_cancelled_task_with_timed_log_is_work() {
        // 見送りでも配下に時刻付きログがあれば実働。着手した時間の記録が
        // 残っている以上、チェックボックスの最終状態だけで捨ててはならない
        let l = lines(&[
            "# 2026-08-01",
            "- [-] [[在庫管理]] を進める",
            "  - 2026-08-01 10:00: 30分触ってやめた",
        ]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(works.len(), 1);
        assert_eq!(works[0].date, "2026-08-01");
    }

    #[test]
    fn test_journal_work_by_tag() {
        let l = lines(&["# 2026-08-01", "- [x] カードを作る #永夜"]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-08-01"]);
        // 前方一致で誤爆しない
        assert!(work_dates(&works, "永夜祭").is_empty());
    }

    #[test]
    fn test_journal_work_uses_log_date_not_file_date() {
        // 時刻付きログは自分の日付を持っているのでそちらを採る
        let l = lines(&[
            "# 2026-08-01",
            "- [ ] [[永夜]] を進める",
            "    - 2026-07-31 22:00: 前日の作業",
        ]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-07-31"]);
    }

    #[test]
    fn test_journal_work_log_must_be_indented_deeper() {
        // タスクと同じ深さのログ行はそのタスクのものではない
        let l = lines(&[
            "- [ ] [[永夜]] を進める",
            "- 2026-08-01 10:00: 無関係のメモ",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_log_after_unlinked_task_is_not_attributed() {
        // 参照を持たないタスク行が来たら文脈を閉じる（次のログを取り違えない）
        let l = lines(&[
            "- [ ] [[永夜]] を進める",
            "- [ ] 無関係のタスク",
            "    - 2026-08-01 10:00: 無関係の作業",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_multiple_refs_on_one_task() {
        let l = lines(&["- [x] [[永夜]] と [[在庫管理]] をまとめて片付けた"]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-08-01"]);
        assert_eq!(work_dates(&works, "在庫管理"), ["2026-08-01"]);
    }

    #[test]
    fn test_journal_work_ignores_code_block() {
        let l = lines(&[
            "```markdown",
            "- [x] [[永夜]] のサンプル記法",
            "```",
            "- [ ] 何もしていない",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_strips_md_extension_from_link() {
        let l = lines(&["- [x] [[永夜.md]] を進めた"]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-08-01"]);
    }

    #[test]
    fn test_journal_work_context_closes_at_heading() {
        // 候補に並べたあと別セクションに時刻メモを書くのは普通の journal の形。
        // 見出しで文脈が閉じないと、候補に載せただけの PJ が実働に化ける
        let l = lines(&[
            "# 2026-08-01",
            "## 今日の候補",
            "- [ ] [[在庫管理]] を始める",
            "",
            "## 記録",
            "",
            "- 定例ミーティング",
            "    - 2026-08-01 10:00-11:00: 進捗共有",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_context_closes_at_sibling_bullet() {
        // 兄弟の箇条書き配下のログは、その上のタスクのものではない
        let l = lines(&[
            "- [ ] [[在庫管理]] を進める",
            "- 打ち合わせメモ",
            "    - 2026-08-01 14:00: 別件",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    #[test]
    fn test_journal_work_survives_blank_line_and_nested_bullet() {
        // 空行やタスク配下のメモを挟んでもそのタスクのログは拾う
        let l = lines(&[
            "- [ ] [[永夜]] を進める",
            "    - メモ: 下書きから",
            "",
            "    - 2026-08-01 10:00: 着手",
        ]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "永夜"), ["2026-08-01"]);
    }

    #[test]
    fn test_journal_work_nested_task_keeps_own_context() {
        // 入れ子のタスクは自分のインデントで文脈を持つ
        let l = lines(&[
            "- [ ] [[親PJ]] を進める",
            "    - [ ] [[子PJ]] の下ごしらえ",
            "        - 2026-08-01 10:00: 子だけ着手",
        ]);
        let works = journal_work(&l, "2026-08-01");
        assert_eq!(work_dates(&works, "子PJ"), ["2026-08-01"]);
        assert!(work_dates(&works, "親PJ").is_empty());
    }

    #[test]
    fn test_journal_work_full_width_space_indent_does_not_attach() {
        // 全角スペース字下げのログはログ行に一致しないので実働にならない。
        // `indent_width` と正規表現のインデントが同じ数え方であることの裏取りでもある
        let l = lines(&[
            "- [ ] [[在庫管理]] を進める",
            "　- 2026-08-01 10:00: 全角スペース字下げ",
        ]);
        assert!(journal_work(&l, "2026-08-01").is_empty());
    }

    // --- collect_document_refs ---

    #[test]
    fn test_collect_document_refs_collects_links_and_tags() {
        let l = lines(&[
            "# 2026-08-01",
            "- [ ] [[在庫管理]] を進める",
            "本文中の #永夜 にも触れた",
        ]);
        let refs = collect_document_refs(&l);
        assert!(refs.contains(&"在庫管理".to_string()));
        assert!(refs.contains(&"永夜".to_string()));
    }

    #[test]
    fn test_collect_document_refs_ignores_code_block() {
        // 記法の例として ``` の中に書いた参照は言及にしない
        let l = lines(&[
            "# 2026-08-01",
            "```markdown",
            "- [ ] [[在庫管理]] を進める例",
            "#永夜 の書き方",
            "```",
        ]);
        assert!(collect_document_refs(&l).is_empty());
    }

    #[test]
    fn test_collect_document_refs_resumes_after_code_block() {
        let l = lines(&["```", "[[コード内]]", "```", "本文の [[在庫管理]]"]);
        assert_eq!(collect_document_refs(&l), ["在庫管理"]);
    }

    #[test]
    fn test_collect_document_refs_superset_of_journal_work_refs() {
        // 実働 ⊆ 言及（I-17）。実働側が拾う参照は必ず言及側にも現れる
        let l = lines(&["# 2026-08-01", "- [x] [[在庫管理]] を進める #永夜"]);
        let doc_refs = collect_document_refs(&l);
        for w in journal_work(&l, "2026-08-01") {
            for r in w.refs {
                assert!(doc_refs.contains(&r), "{r} が言及側に無い");
            }
        }
    }
}
