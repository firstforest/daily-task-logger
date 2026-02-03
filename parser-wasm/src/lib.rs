use regex::Regex;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParsedTask {
    is_completed: bool,
    text: String,
    line: usize,
    log: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParsedTaskWithDate {
    is_completed: bool,
    text: String,
    line: usize,
    log: String,
    date: String,
}

struct CurrentTask {
    indent: usize,
    completed: bool,
    text: String,
    line: usize,
}

#[wasm_bindgen(js_name = "parseTasks")]
pub fn parse_tasks(lines_js: JsValue, target_date: &str) -> JsValue {
    let lines: Vec<String> = serde_wasm_bindgen::from_value(lines_js).unwrap_or_default();

    let task_re = Regex::new(r"^(\s*)-\s*\[([ x])\]\s*(.*)").unwrap();
    let date_re = Regex::new(r"^(\s*)-\s*(\d{4}-\d{2}-\d{2}):\s*(.*)").unwrap();

    let mut tasks: Vec<ParsedTask> = Vec::new();
    let mut current_task: Option<CurrentTask> = None;

    for (i, text) in lines.iter().enumerate() {
        if let Some(caps) = task_re.captures(text) {
            current_task = Some(CurrentTask {
                indent: caps[1].len(),
                completed: &caps[2] == "x",
                text: caps[3].to_string(),
                line: i,
            });
            continue;
        }

        if let Some(caps) = date_re.captures(text) {
            if let Some(ref ct) = current_task {
                let date_indent = caps[1].len();
                let date_str = &caps[2];
                let log_content = &caps[3];

                if date_str == target_date && date_indent > ct.indent {
                    tasks.push(ParsedTask {
                        is_completed: ct.completed,
                        text: ct.text.clone(),
                        line: ct.line,
                        log: log_content.to_string(),
                    });
                }
            }
        }
    }

    serde_wasm_bindgen::to_value(&tasks).unwrap()
}

#[wasm_bindgen(js_name = "parseTasksAllDates")]
pub fn parse_tasks_all_dates(lines_js: JsValue) -> JsValue {
    let lines: Vec<String> = serde_wasm_bindgen::from_value(lines_js).unwrap_or_default();

    let task_re = Regex::new(r"^(\s*)-\s*\[([ x])\]\s*(.*)").unwrap();
    let date_re = Regex::new(r"^(\s*)-\s*(\d{4}-\d{2}-\d{2}):\s*(.*)").unwrap();

    let mut tasks: Vec<ParsedTaskWithDate> = Vec::new();
    let mut current_task: Option<CurrentTask> = None;
    let mut current_task_has_log = false;

    for (i, text) in lines.iter().enumerate() {
        if let Some(caps) = task_re.captures(text) {
            if let Some(ref ct) = current_task {
                if !current_task_has_log {
                    tasks.push(ParsedTaskWithDate {
                        is_completed: ct.completed,
                        text: ct.text.clone(),
                        line: ct.line,
                        log: String::new(),
                        date: String::new(),
                    });
                }
            }
            current_task = Some(CurrentTask {
                indent: caps[1].len(),
                completed: &caps[2] == "x",
                text: caps[3].to_string(),
                line: i,
            });
            current_task_has_log = false;
            continue;
        }

        if let Some(caps) = date_re.captures(text) {
            if let Some(ref ct) = current_task {
                let date_indent = caps[1].len();
                let date_str = &caps[2];
                let log_content = &caps[3];

                if date_indent > ct.indent {
                    tasks.push(ParsedTaskWithDate {
                        is_completed: ct.completed,
                        text: ct.text.clone(),
                        line: ct.line,
                        log: log_content.to_string(),
                        date: date_str.to_string(),
                    });
                    current_task_has_log = true;
                }
            }
        }
    }

    if let Some(ref ct) = current_task {
        if !current_task_has_log {
            tasks.push(ParsedTaskWithDate {
                is_completed: ct.completed,
                text: ct.text.clone(),
                line: ct.line,
                log: String::new(),
                date: String::new(),
            });
        }
    }

    serde_wasm_bindgen::to_value(&tasks).unwrap()
}
