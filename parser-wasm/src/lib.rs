use wasm_bindgen::prelude::*;

pub use parser_core::{
    FileInput, ParsedTask, ParsedTaskWithDate, ScheduleEntry, TreeDateGroup, TreeFileGroup,
    TreeTaskData,
};
pub use parser_core::wiki_link::{NormalizedName, WikiLinkMatch};

// === WASM exports ===

#[wasm_bindgen(js_name = "parseTasks")]
pub fn parse_tasks(lines_js: JsValue, target_date: &str) -> JsValue {
    let lines: Vec<String> = serde_wasm_bindgen::from_value(lines_js).unwrap_or_default();
    let tasks = parser_core::parse_tasks_internal(&lines, target_date);
    serde_wasm_bindgen::to_value(&tasks).unwrap()
}

#[wasm_bindgen(js_name = "parseTasksAllDates")]
pub fn parse_tasks_all_dates(lines_js: JsValue) -> JsValue {
    let lines: Vec<String> = serde_wasm_bindgen::from_value(lines_js).unwrap_or_default();
    let tasks = parser_core::parse_all_dates_internal(&lines);
    serde_wasm_bindgen::to_value(&tasks).unwrap()
}

#[wasm_bindgen(js_name = "buildTreeData")]
pub fn build_tree_data(files_js: JsValue, today_str: &str) -> JsValue {
    let files: Vec<FileInput> = serde_wasm_bindgen::from_value(files_js).unwrap_or_default();
    let result = parser_core::build_tree_data_internal(files, today_str);
    serde_wasm_bindgen::to_value(&result).unwrap()
}

#[wasm_bindgen(js_name = "buildScheduleData")]
pub fn build_schedule_data(files_js: JsValue, target_date: &str) -> JsValue {
    let files: Vec<FileInput> = serde_wasm_bindgen::from_value(files_js).unwrap_or_default();
    let result = parser_core::build_schedule_data_internal(files, target_date);
    serde_wasm_bindgen::to_value(&result).unwrap()
}

#[wasm_bindgen(js_name = "extractTags")]
pub fn extract_tags(text: &str) -> JsValue {
    let tags = parser_core::extract_tags(text);
    serde_wasm_bindgen::to_value(&tags).unwrap()
}

#[wasm_bindgen(js_name = "extractFileTags")]
pub fn extract_file_tags(lines_js: JsValue, file_name: &str) -> JsValue {
    let lines: Vec<String> = serde_wasm_bindgen::from_value(lines_js).unwrap_or_default();
    let tags = parser_core::extract_file_tags(&lines, file_name);
    serde_wasm_bindgen::to_value(&tags).unwrap()
}

#[wasm_bindgen(js_name = "parseWikiLinks")]
pub fn parse_wiki_links(text: &str) -> JsValue {
    let links = parser_core::wiki_link::parse_wiki_links(text);
    serde_wasm_bindgen::to_value(&links).unwrap()
}

#[wasm_bindgen(js_name = "normalizeWikiName")]
pub fn normalize_wiki_name(raw: &str) -> JsValue {
    let normalized = parser_core::wiki_link::normalize_wiki_name(raw);
    serde_wasm_bindgen::to_value(&normalized).unwrap()
}

#[wasm_bindgen(js_name = "resolveWikiLink")]
pub fn resolve_wiki_link(name: &str, candidate_paths: Vec<String>) -> Option<String> {
    let candidates: Vec<std::path::PathBuf> =
        candidate_paths.into_iter().map(std::path::PathBuf::from).collect();
    parser_core::wiki_link::resolve(name, &candidates)
        .map(|p| p.to_string_lossy().to_string())
}

#[wasm_bindgen(js_name = "wikiLinkCreatePath")]
pub fn wiki_link_create_path(name: &str, is_journal: bool, taski_home: &str) -> String {
    parser_core::wiki_link::wiki_link_create_path(
        name,
        is_journal,
        std::path::Path::new(taski_home),
    )
    .to_string_lossy()
    .to_string()
}

#[wasm_bindgen(js_name = "wikiLinkInitialContent")]
pub fn wiki_link_initial_content(name: &str) -> String {
    parser_core::wiki_link::wiki_link_initial_content(name)
}
