use calamine::{open_workbook_auto, Reader};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use std::time::Duration;
use xlsxwriter::{Format, Workbook};

const MAX_TRANSLATIONS_PER_REQUEST: usize = 200;
const DEFAULT_OPENAI_BASE_URL: &str =
    "https://api.openai.com/v1/chat/completions";
const DEFAULT_OPENAI_MODEL: &str = "gpt-5-chat-latest";
// const DEFAULT_OPENAI_MODEL: &str = "gpt-5.2-2025-12-11";
const HEADER_NOTE: &str = "1、请上传小于 9999 条，99 MB的 EXCEL 文件。\n2、请在语言列增加对应的翻译，实现多语言的翻译配置。修改文案或清空文案都会覆盖原始数据，默认语言必须录入对应的翻译，否则会导致该行数据导入失败。新增加行数据将不会新增词条。\n3、请勿变更列数据的位置，请勿删除此行。";

pub(crate) struct TranslateResult {
    pub translated_count: usize,
    pub skipped_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TranslateProgress {
    pub total: usize,
    pub processed: usize,
    pub translated: usize,
    pub skipped: usize,
}

#[derive(Debug)]
struct PendingRow {
    row_index: usize,
    text: String,
}

#[derive(Debug)]
struct WorkbookData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug)]
struct AiConfig {
    api_key: String,
    base_url: String,
    model: String,
}

impl AiConfig {
    fn from_env() -> Result<Self, String> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| "Missing OPENAI_API_KEY environment variable".to_string())?;
        let base_url =
            env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_OPENAI_BASE_URL.to_string());
        let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_OPENAI_MODEL.to_string());
        Ok(Self {
            api_key,
            base_url,
            model,
        })
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: String,
}

pub(crate) async fn ai_translate_with_progress<F>(
    source_file: &str,
    source_lang: &str,
    target_lang: &str,
    output_file: &str,
    mut on_progress: F,
) -> Result<TranslateResult, String>
where
    F: FnMut(TranslateProgress) + Send,
{
    if source_lang == target_lang {
        return Err("Source language and target language cannot be the same".to_string());
    }

    let current_dir = env::current_dir().map_err(|e| format!("Failed to get current dir: {e}"))?;
    let source_path = current_dir.join(source_file);
    let mut workbook_data = read_workbook_data(source_path)?;

    let source_column = workbook_data
        .headers
        .iter()
        .position(|h| h == source_lang)
        .ok_or_else(|| format!("Source language column not found: {source_lang}"))?;
    let target_column = workbook_data
        .headers
        .iter()
        .position(|h| h == target_lang)
        .ok_or_else(|| format!("Target language column not found: {target_lang}"))?;

    let mut pending_rows: Vec<PendingRow> = Vec::new();
    let mut skipped_count = 0usize;

    for (row_index, row) in workbook_data.rows.iter().enumerate() {
        let source_value = row
            .get(source_column)
            .map(|v| v.trim())
            .unwrap_or_default()
            .to_string();
        let target_value = row
            .get(target_column)
            .map(|v| v.trim())
            .unwrap_or_default()
            .to_string();

        if source_value.is_empty() || !target_value.is_empty() {
            skipped_count += 1;
            continue;
        }

        pending_rows.push(PendingRow {
            row_index,
            text: row[source_column].clone(),
        });
    }

    let ai_config = AiConfig::from_env()?;
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let total = pending_rows.len();
    let mut translated_count = 0usize;
    let mut processed_count = 0usize;

    on_progress(TranslateProgress {
        total,
        processed: processed_count,
        translated: translated_count,
        skipped: skipped_count,
    });

    for chunk in pending_rows.chunks(MAX_TRANSLATIONS_PER_REQUEST) {
        let source_texts: Vec<String> = chunk.iter().map(|item| item.text.clone()).collect();
        log::debug!(
            "Sending AI translation chunk: source_lang={}, target_lang={}, items={}",
            source_lang,
            target_lang,
            source_texts.len()
        );
        let translated_texts =
            translate_chunk(&client, &ai_config, source_lang, target_lang, &source_texts).await?;

        if translated_texts.len() != chunk.len() {
            return Err(format!(
                "AI output length mismatch, expected {}, got {}",
                chunk.len(),
                translated_texts.len()
            ));
        }

        for (item, translated) in chunk.iter().zip(translated_texts.into_iter()) {
            if let Some(cell) = workbook_data
                .rows
                .get_mut(item.row_index)
                .and_then(|r| r.get_mut(target_column))
            {
                *cell = translated;
                translated_count += 1;
            }
        }

        processed_count += chunk.len();
        on_progress(TranslateProgress {
            total,
            processed: processed_count,
            translated: translated_count,
            skipped: skipped_count,
        });
    }

    write_workbook_data(output_file, &workbook_data.headers, &workbook_data.rows)?;
    Ok(TranslateResult {
        translated_count,
        skipped_count,
    })
}

fn read_workbook_data(path: PathBuf) -> Result<WorkbookData, String> {
    let mut workbook = open_workbook_auto(&path)
        .map_err(|e| format!("Failed to open workbook {}: {e}", path.display()))?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| "Workbook has no worksheet".to_string())?;
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Failed to read worksheet {sheet_name}: {e}"))?;

    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    for (row_index, row) in range.rows().enumerate() {
        if row_index == 1 {
            headers = row.iter().map(|c| c.to_string()).collect();
        } else if row_index > 1 {
            if headers.is_empty() {
                continue;
            }
            let mut values = vec![String::new(); headers.len()];
            for (col_index, value) in values.iter_mut().enumerate().take(headers.len()) {
                if let Some(cell) = row.get(col_index) {
                    *value = cell.to_string();
                }
            }
            rows.push(values);
        }
    }

    if headers.is_empty() {
        return Err("No headers found in row 2".to_string());
    }

    Ok(WorkbookData { headers, rows })
}

fn write_workbook_data(
    output_file: &str,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<(), String> {
    if headers.is_empty() {
        return Err("Cannot write workbook with empty headers".to_string());
    }

    let workbook =
        Workbook::new(output_file).map_err(|e| format!("Failed to create workbook: {e}"))?;
    let mut sheet = workbook
        .add_worksheet(Some("全部"))
        .map_err(|e| format!("Failed to add worksheet: {e}"))?;

    sheet
        .merge_range(
            0,
            0,
            0,
            headers.len() as u16 - 1,
            HEADER_NOTE,
            Some(&Format::new().set_bold().set_text_wrap()),
        )
        .map_err(|e| format!("Failed to write header note: {e}"))?;
    sheet
        .set_row(0, 50.0, None)
        .map_err(|e| format!("Failed to set first row style: {e}"))?;

    for (column_index, header) in headers.iter().enumerate() {
        sheet
            .write_string(1, column_index as u16, header, None)
            .map_err(|e| format!("Failed to write header {header}: {e}"))?;
    }

    for (row_index, row) in rows.iter().enumerate() {
        let output_row = row_index as u32 + 2;
        for (column_index, value) in row.iter().enumerate().take(headers.len()) {
            sheet
                .write_string(output_row, column_index as u16, value, None)
                .map_err(|e| {
                    format!("Failed to write row {output_row}, col {column_index}: {e}")
                })?;
        }
    }

    workbook
        .close()
        .map_err(|e| format!("Failed to close workbook: {e}"))?;
    Ok(())
}

async fn translate_chunk(
    client: &Client,
    config: &AiConfig,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
) -> Result<Vec<String>, String> {
    if source_texts.is_empty() {
        return Ok(Vec::new());
    }

    let system_prompt = format!(
        "请将之后发送的{}翻译为{}, 不要输出多余内容. 不需要翻译 `{{}}` 内部的内容",
        source_lang, target_lang
    );
    let source_payload = serde_json::to_string(source_texts)
        .map_err(|e| format!("Failed to serialize source texts: {e}"))?;
    let user_prompt = format!(
        "请翻译发送内容的 JSON 数组内容，并遵守：\n1. 只输出 JSON 数组\n2. 输出元素数量与输入一致\n3. 保持顺序一致\n\n```json\n{}\n```",
        source_payload
    );

    let request = ChatCompletionRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt,
            },
        ],
    };
    let request_body = serde_json::to_string_pretty(&request)
        .unwrap_or_else(|_| "<failed to serialize request body>".to_string());
    log::debug!(
        "AI request: url={}, model={}, body={}",
        config.base_url,
        config.model,
        request_body
    );

    let response = client
        .post(&config.base_url)
        .bearer_auth(&config.api_key)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("AI request failed: {e}"))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read AI response body: {e}"))?;
    log::debug!(
        "AI response: status={}, body={}",
        status.as_u16(),
        response_text
    );

    if !status.is_success() {
        return Err(format!(
            "AI request returned non-success status {}: {}",
            status.as_u16(),
            response_text
        ));
    }

    let completion: ChatCompletionResponse = serde_json::from_str(&response_text)
        .map_err(|e| format!("Failed to parse AI response JSON: {e}; body: {response_text}"))?;
    let raw_content = completion
        .choices
        .first()
        .ok_or_else(|| "AI response choices are empty".to_string())?
        .message
        .content
        .trim()
        .to_string();

    parse_translated_array(&raw_content, source_texts.len())
}

fn parse_translated_array(raw_content: &str, expected_len: usize) -> Result<Vec<String>, String> {
    let cleaned_content = strip_code_fence(raw_content);
    let translated: Vec<String> = serde_json::from_str(cleaned_content.trim()).map_err(|e| {
        format!(
            "Failed to parse translated output as JSON array: {e}; output: {}",
            raw_content
        )
    })?;

    if translated.len() != expected_len {
        return Err(format!(
            "Translated item count mismatch, expected {}, got {}",
            expected_len,
            translated.len()
        ));
    }

    Ok(translated)
}

fn strip_code_fence(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut lines = trimmed.lines();
    let _ = lines.next();
    let mut inner_lines: Vec<&str> = lines.collect();

    if inner_lines
        .last()
        .map(|line| line.trim_start().starts_with("```"))
        .unwrap_or(false)
    {
        inner_lines.pop();
    }

    inner_lines.join("\n")
}
