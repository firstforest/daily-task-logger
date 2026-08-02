//! 行の文法（docs/syntax.md §3）と、帰属を伴う走査（docs/design.md §4）。
//!
//! **同じ文法の正規表現をここ以外に書かない**（design.md P4）。以前はタスク行の
//! 正規表現が 5 本、ログ行が 5 本あり、記法を 1 つ変えるたびに全部を揃える必要が
//! あった。実際に揃え漏れが起きている。
//!
//! **帰属規則も 1 本にまとめてある**（design.md G-13）。以前は `pj::journal_work`
//! だけが厳しい規則（浅い非空行で文脈を閉じる）を使い、他の経路はインデントだけを
//! 見る緩い規則だったため、同じ 1 行が `taski list` では直前のタスクのログになり、
//! `taski pj` の実働判定では帰属しない、ということが起きていた。
//! domain.md §1 の `attach` は厳しい方なので、そちらに揃えている。

use std::sync::OnceLock;

use regex::Regex;

use crate::TaskStatus;

// === 行の文法（syntax.md §3 の EBNF）===
//
// 行の構造を区切る空白は `\s` ではなく `[ \t]` で書く。`regex` クレートの `\s` は
// Unicode 空白にも一致するので、`\s` のままだと全角スペース（U+3000）で字下げした行が
// タスク・ログとして通ってしまう。記法上の空白は半角スペースとタブだけ（syntax.md §3）。

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*(?:```|~~~)").unwrap())
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(#{1,6})[ \t]+(.*)").unwrap())
}

/// タスク行。`- [[ノート名]] ...` は `[` の次が `[` なのでマーカーの 1 文字に一致しない。
fn task_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([ \t]*)-[ \t]*\[([ x-])\][ \t]*(.*)").unwrap())
}

/// ログ行。時刻・終了時刻は任意で、どちらもキャプチャする。
///
/// 時刻を捨てる経路（`ParsedTaskWithDate` など）も同じ 1 本を引き、要らなければ
/// 受け取ったあとで無視する。時刻ありだけを拾う狭い正規表現を別に持つと、記法を
/// 変えたときに片方だけ直す事故が起きる（実働判定はこれで `timed_log_re` を
/// 引いていた）。
fn log_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^([ \t]*)-[ \t]*(\d{4}-\d{2}-\d{2})(?:[ \t]+(\d{1,2}:\d{2})(?:-(\d{1,2}:\d{2}))?)?:[ \t]*(.*)",
        )
        .unwrap()
    })
}

/// トップレベル時刻メモ。インデント不可、区切りの空白はそれぞれちょうど 1 個。
fn time_memo_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^-[ \t](\d{1,2}:\d{2}):[ \t](.+)").unwrap())
}

/// 箇条書き。`task` にも `log` にも一致しなかった `- ` 行がここに落ちる。
fn bullet_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([ \t]*)-[ \t]+(.*)").unwrap())
}

/// 行の種別（syntax.md §3）。判定は EBNF の並び順に上から行い、最初に一致したものを採る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind<'a> {
    Fence,
    Heading {
        level: usize,
        text: &'a str,
    },
    Task {
        indent: usize,
        status: TaskStatus,
        text: &'a str,
    },
    Log {
        indent: usize,
        date: &'a str,
        time: Option<&'a str>,
        end_time: Option<&'a str>,
        text: &'a str,
    },
    TimeMemo {
        time: &'a str,
        text: &'a str,
    },
    Bullet {
        indent: usize,
        text: &'a str,
    },
    Other,
}

/// 1 行の種別を決める。
pub fn classify(line: &str) -> LineKind<'_> {
    if fence_re().is_match(line) {
        return LineKind::Fence;
    }
    if let Some(caps) = heading_re().captures(line) {
        return LineKind::Heading {
            level: caps[1].len(),
            text: caps.get(2).unwrap().as_str(),
        };
    }
    if let Some(caps) = task_re().captures(line) {
        return LineKind::Task {
            indent: caps[1].len(),
            status: TaskStatus::from_marker(&caps[2]),
            text: caps.get(3).unwrap().as_str(),
        };
    }
    if let Some(caps) = log_re().captures(line) {
        return LineKind::Log {
            indent: caps[1].len(),
            date: caps.get(2).unwrap().as_str(),
            time: caps.get(3).map(|m| m.as_str()),
            end_time: caps.get(4).map(|m| m.as_str()),
            text: caps.get(5).unwrap().as_str(),
        };
    }
    if let Some(caps) = time_memo_re().captures(line) {
        return LineKind::TimeMemo {
            time: caps.get(1).unwrap().as_str(),
            text: caps.get(2).unwrap().as_str(),
        };
    }
    if let Some(caps) = bullet_re().captures(line) {
        return LineKind::Bullet {
            indent: caps[1].len(),
            text: caps.get(2).unwrap().as_str(),
        };
    }
    LineKind::Other
}

/// 行頭の空白幅。タスク行・ログ行の `^([ \t]*)` キャプチャと同じ数え方に揃える。
///
/// `trim_start` は Unicode 空白まで落とすので使えない。全角スペースで字下げした行は
/// 正規表現側ではインデント 0 になるため、ここで 3（UTF-8 のバイト数）を返すと
/// 帰属の判定が食い違う。
pub fn indent_width(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// 文書のどこかに、指定日と一致するレベル 1 の日付見出しがあるか（syntax.md §3.3）。
///
/// トップレベル時刻メモの発火条件。フェンス内は数えない（記法の例としてコードブロックに
/// 書いた `# YYYY-MM-DD` で時刻メモを発火させないため）。
pub fn has_date_heading(lines: &[String], date: &str) -> bool {
    let mut in_code = false;
    for line in lines {
        match classify(line) {
            LineKind::Fence => in_code = !in_code,
            LineKind::Heading { level: 1, text } if !in_code && text.starts_with(date) => {
                return true
            }
            _ => {}
        }
    }
    false
}

// === 走査 ===

/// 走査中のタスク文脈（design.md §4 の `TaskCtx`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCtx {
    pub indent: usize,
    pub status: TaskStatus,
    pub text: String,
    pub line: usize,
    /// 祖先見出しのスタック
    pub context: Vec<String>,
}

/// 走査が生む出来事。行の出現順に渡される。
pub enum Event<'a> {
    /// タスク行に出会った。
    Task(&'a TaskCtx),
    /// タスク文脈が閉じた（次のタスク行・浅い非空行・文書末）。
    ///
    /// `had_log` はそのタスクにログが 1 件でも帰属したか。ログを 1 件も持たない
    /// タスクを「日付なし」として出すのはここが契機になる（design.md I-2）。
    TaskClosed { task: &'a TaskCtx, had_log: bool },
    /// ログ行。`task` が `Some` ならそのタスクに帰属する（domain.md §1 の `attach`）。
    Log {
        task: Option<&'a TaskCtx>,
        line: usize,
        indent: usize,
        date: &'a str,
        time: Option<&'a str>,
        end_time: Option<&'a str>,
        text: &'a str,
    },
    /// トップレベル時刻メモ。
    TimeMemo {
        line: usize,
        time: &'a str,
        text: &'a str,
    },
    /// 箇条書き（タスクにもログにも一致しなかった `- ` 行）。
    Bullet {
        task: Option<&'a TaskCtx>,
        line: usize,
        indent: usize,
        text: &'a str,
    },
}

fn close<F>(current: &mut Option<TaskCtx>, had_log: &mut bool, on: &mut F)
where
    F: FnMut(Event<'_>),
{
    if let Some(ctx) = current.take() {
        on(Event::TaskClosed {
            task: &ctx,
            had_log: *had_log,
        });
    }
    *had_log = false;
}

/// 行を走査し、帰属を解決しながら出来事を渡す（design.md §4）。
///
/// 帰属の規則は 1 つだけで、経路によって変わらない（domain.md §1）。
///
/// - ログがタスクに付くのは、インデントが**厳密に深い**とき。同じ深さの
///   `- YYYY-MM-DD: ...` はタスクの兄弟であってログではない。
/// - 空でない行のインデントがタスク以下になった時点でタスク文脈は閉じる。
///   見出し・兄弟の箇条書き・段落が該当する。**空行だけでは閉じない**（ログの間に
///   空行を挟む書き方を落とさないため）。
/// - フェンス行は文脈を捨てない。タスクの配下にコードブロックを挟んでもログの帰属は
///   切れる必要がない（syntax.md §2.3）。フェンスの内側は全解析から除外する。
pub fn scan<F>(lines: &[String], mut on: F)
where
    F: FnMut(Event<'_>),
{
    let mut in_code = false;
    let mut heads: Vec<String> = Vec::new();
    let mut current: Option<TaskCtx> = None;
    let mut had_log = false;

    for (i, raw) in lines.iter().enumerate() {
        let kind = classify(raw);

        if kind == LineKind::Fence {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }

        // その行がタスク文脈を閉じるか。空でない行のインデントがタスク以下なら閉じる。
        // 見出しと時刻メモは行頭に空白を置けないので常にインデント 0 として扱う。
        let closes = |current: &Option<TaskCtx>, indent: usize| {
            current.as_ref().is_some_and(|c| indent <= c.indent)
        };

        match kind {
            LineKind::Fence => unreachable!("フェンスは上で処理済み"),
            LineKind::Heading { level, text } => {
                heads.truncate(level - 1);
                heads.push(text.to_string());
                close(&mut current, &mut had_log, &mut on);
            }
            LineKind::Task {
                indent,
                status,
                text,
            } => {
                close(&mut current, &mut had_log, &mut on);
                current = Some(TaskCtx {
                    indent,
                    status,
                    text: text.to_string(),
                    line: i,
                    context: heads.clone(),
                });
                on(Event::Task(current.as_ref().expect("直前に代入している")));
            }
            LineKind::Log {
                indent,
                date,
                time,
                end_time,
                text,
            } => {
                if closes(&current, indent) {
                    close(&mut current, &mut had_log, &mut on);
                }
                let attached = current.is_some();
                on(Event::Log {
                    task: current.as_ref(),
                    line: i,
                    indent,
                    date,
                    time,
                    end_time,
                    text,
                });
                had_log |= attached;
            }
            LineKind::TimeMemo { time, text } => {
                close(&mut current, &mut had_log, &mut on);
                on(Event::TimeMemo {
                    line: i,
                    time,
                    text,
                });
            }
            LineKind::Bullet { indent, text } => {
                if closes(&current, indent) {
                    close(&mut current, &mut had_log, &mut on);
                }
                on(Event::Bullet {
                    task: current.as_ref(),
                    line: i,
                    indent,
                    text,
                });
            }
            LineKind::Other => {
                if !raw.trim().is_empty() && closes(&current, indent_width(raw)) {
                    close(&mut current, &mut had_log, &mut on);
                }
            }
        }
    }

    close(&mut current, &mut had_log, &mut on);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// 走査結果を「帰属したログの (タスク本文, 日付)」に潰す。帰属の確認用。
    fn attached(items: &[&str]) -> Vec<(String, String)> {
        let mut got = Vec::new();
        scan(&lines(items), |ev| {
            if let Event::Log {
                task: Some(t),
                date,
                ..
            } = ev
            {
                got.push((t.text.clone(), date.to_string()));
            }
        });
        got
    }

    // --- classify ---

    #[test]
    fn test_classify_task_markers() {
        assert!(matches!(
            classify("- [x] 済み"),
            LineKind::Task {
                status: TaskStatus::Completed,
                ..
            }
        ));
        assert!(matches!(
            classify("- [-] 見送り"),
            LineKind::Task {
                status: TaskStatus::Cancelled,
                ..
            }
        ));
    }

    #[test]
    fn test_classify_wiki_link_bullet_is_not_a_task() {
        // `[` の次が `[` なのでマーカーの 1 文字に一致しない（syntax.md §7）
        assert!(matches!(
            classify("- [[ノート名]] について"),
            LineKind::Bullet { .. }
        ));
    }

    #[test]
    fn test_classify_log_captures_times() {
        let got = classify("    - 2026-08-01 10:00-11:30: 推敲");
        assert_eq!(
            got,
            LineKind::Log {
                indent: 4,
                date: "2026-08-01",
                time: Some("10:00"),
                end_time: Some("11:30"),
                text: "推敲",
            }
        );
    }

    #[test]
    fn test_classify_log_without_time() {
        assert!(matches!(
            classify("- 2026-08-01: 構成を決めた"),
            LineKind::Log { time: None, .. }
        ));
    }

    #[test]
    fn test_classify_time_memo_requires_top_level() {
        assert!(matches!(classify("- 09:00: 朝会"), LineKind::TimeMemo { .. }));
        // インデントされた時刻メモは時刻メモではない。日付が無いのでログでもなく、
        // 単なる箇条書きになる（syntax.md §3.4）
        assert!(matches!(classify("  - 09:00: 朝会"), LineKind::Bullet { .. }));
    }

    #[test]
    fn test_classify_full_width_space_is_not_indent() {
        // 記法上の空白は半角スペースとタブだけ（syntax.md §3）
        assert!(matches!(classify("　- [ ] 本文"), LineKind::Other));
        assert!(matches!(classify("-　[ ] 本文"), LineKind::Other));
    }

    #[test]
    fn test_classify_heading_level() {
        assert_eq!(
            classify("## 見出し"),
            LineKind::Heading {
                level: 2,
                text: "見出し"
            }
        );
    }

    // --- has_date_heading ---

    #[test]
    fn test_date_heading_ignores_level_two() {
        assert!(!has_date_heading(&lines(&["## 2026-08-02"]), "2026-08-02"));
        assert!(has_date_heading(&lines(&["# 2026-08-02 土曜"]), "2026-08-02"));
    }

    #[test]
    fn test_date_heading_ignores_fenced() {
        let l = lines(&["```markdown", "# 2026-08-02", "```"]);
        assert!(!has_date_heading(&l, "2026-08-02"));
    }

    // --- 帰属 ---

    #[test]
    fn test_attach_requires_strictly_deeper_indent() {
        assert_eq!(
            attached(&["- [ ] タスク", "    - 2026-08-01: 配下"]),
            vec![("タスク".to_string(), "2026-08-01".to_string())]
        );
        // 同じ深さの `- YYYY-MM-DD: ...` はタスクの兄弟であってログではない
        assert!(attached(&["- [ ] タスク", "- 2026-08-01: 兄弟"]).is_empty());
    }

    #[test]
    fn test_sibling_log_closes_the_context() {
        // 兄弟のログは「浅い非空行」でもあるので、そこでタスク文脈が閉じる。
        // 以降に深いログを書いても、もうそのタスクには付かない
        let got = attached(&[
            "- [ ] タスク",
            "- 2026-08-01: 兄弟",
            "    - 2026-08-02: 兄弟の配下のつもりの行",
        ]);
        assert!(got.is_empty());
    }

    #[test]
    fn test_attach_survives_blank_line_and_fence() {
        let got = attached(&[
            "- [ ] タスク",
            "",
            "    ```rust",
            "    fn main() {}",
            "    ```",
            "",
            "    - 2026-08-01: ログ",
        ]);
        assert_eq!(got, vec![("タスク".to_string(), "2026-08-01".to_string())]);
    }

    #[test]
    fn test_attach_closes_at_shallow_heading() {
        let got = attached(&["- [ ] タスク", "## 別の見出し", "    - 2026-08-01: ログ"]);
        assert!(got.is_empty(), "見出しを挟んだら帰属しない");
    }

    #[test]
    fn test_attach_closes_at_shallow_paragraph() {
        let got = attached(&["- [ ] タスク", "段落テキスト", "    - 2026-08-01: ログ"]);
        assert!(got.is_empty(), "浅い非空行を挟んだら帰属しない");
    }

    #[test]
    fn test_attach_survives_deeper_paragraph() {
        let got = attached(&["- [ ] タスク", "    補足の段落", "    - 2026-08-01: ログ"]);
        assert_eq!(got, vec![("タスク".to_string(), "2026-08-01".to_string())]);
    }

    #[test]
    fn test_attach_nested_task_takes_over() {
        let got = attached(&[
            "- [ ] 親",
            "    - [ ] 子",
            "        - 2026-08-01: ログ",
        ]);
        assert_eq!(got, vec![("子".to_string(), "2026-08-01".to_string())]);
    }

    #[test]
    fn test_attach_ignores_fenced_lines() {
        let got = attached(&["```", "- [ ] タスク", "    - 2026-08-01: ログ", "```"]);
        assert!(got.is_empty());
    }

    // --- TaskClosed ---

    #[test]
    fn test_task_closed_reports_whether_it_had_a_log() {
        let mut got: Vec<(String, bool)> = Vec::new();
        let l = lines(&[
            "- [ ] ログあり",
            "    - 2026-08-01: ログ",
            "- [ ] ログなし",
        ]);
        scan(&l, |ev| {
            if let Event::TaskClosed { task, had_log } = ev {
                got.push((task.text.clone(), had_log));
            }
        });
        assert_eq!(
            got,
            vec![("ログあり".to_string(), true), ("ログなし".to_string(), false)]
        );
    }

    #[test]
    fn test_task_closed_fires_at_end_of_document() {
        let mut count = 0;
        scan(&lines(&["- [ ] 最後のタスク"]), |ev| {
            if matches!(ev, Event::TaskClosed { .. }) {
                count += 1;
            }
        });
        assert_eq!(count, 1);
    }

    // --- 見出しの文脈 ---

    #[test]
    fn test_heading_stack_becomes_task_context() {
        let mut got: Vec<Vec<String>> = Vec::new();
        let l = lines(&["# 大", "## 中", "- [ ] タスク", "### 小", "- [ ] 別"]);
        scan(&l, |ev| {
            if let Event::Task(t) = ev {
                got.push(t.context.clone());
            }
        });
        assert_eq!(
            got,
            vec![
                vec!["大".to_string(), "中".to_string()],
                vec!["大".to_string(), "中".to_string(), "小".to_string()],
            ]
        );
    }
}
