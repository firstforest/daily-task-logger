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

use regex::Regex;
use serde::Serialize;

/// 「次の予定」セクションの見出し名。
pub const SECTION_NEXT: &str = "次の予定";
/// 「ログ」セクションの見出し名。
pub const SECTION_LOG: &str = "ログ";
/// 「オープンタスク」セクションの見出し名。
pub const SECTION_BACKLOG: &str = "オープンタスク";

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

fn heading_re() -> Regex {
    Regex::new(r"^(#{1,6})\s+(.*)").unwrap()
}

fn fence_re() -> Regex {
    Regex::new(r"^\s*(```|~~~)").unwrap()
}

/// タスク行（`- [ ]` / `- [x]` / `- [-]`）。`- [[ノート名]]` は `[` の次が `[` なので一致しない。
fn task_re() -> Regex {
    Regex::new(r"^\s*-\s*\[([ x-])\]\s*(.*)").unwrap()
}

/// ログ行（`- YYYY-MM-DD: 内容`）。時刻・時間範囲付きも許容する。
fn log_re() -> Regex {
    Regex::new(
        r"^\s*-\s*(\d{4}-\d{2}-\d{2})(?:\s+\d{1,2}:\d{2}(?:-\d{1,2}:\d{2})?)?:\s*(.*)",
    )
    .unwrap()
}

/// 箇条書き行（`- 内容`）。
fn bullet_re() -> Regex {
    Regex::new(r"^\s*-\s+(.*)").unwrap()
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
    let duration = Regex::new(r"^\d+\s*(分|時間)$").unwrap();
    let weight = Regex::new(r"^[軽重]$").unwrap();
    let context = Regex::new(r"^@.+$").unwrap();
    let deadline = Regex::new(r"^(締切)?\d{1,2}-\d{1,2}$").unwrap();

    let e = element.trim();
    duration.is_match(e) || weight.is_match(e) || context.is_match(e) || deadline.is_match(e)
}

/// 行末の `（45分・重・@PC）` を判断メタデータとして本文から分離する。
///
/// メタデータでなければ `None`。単なる注記（`（仮）`）や列挙（`（髪・服・顔）`）を
/// 誤検出しないよう、括弧の中身を `・,、` で分割していずれかの要素が
/// メタデータの書式に一致することを条件にする。
pub fn split_decision_meta(text: &str) -> Option<(String, String)> {
    let re = Regex::new(r"^(.*)[（(]([^（）()]{1,60})[）)]\s*$").unwrap();
    let caps = re.captures(text)?;
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
}
