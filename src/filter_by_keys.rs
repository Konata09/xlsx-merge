use calamine::{open_workbook_auto, Error, Reader};
use glob::GlobError;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use xlsxwriter::{Format, Workbook, XlsxError};

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FileStatus {
    VbaError(Error),
    RangeError(Error),
    Glob(GlobError),
}

/// Get headers from Excel file (excluding the key column)
pub(crate) fn get_headers(source_file: &str) -> Result<Vec<String>, FileStatus> {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let source_path = current_dir.join(source_file);
    read_headers(source_path)
}

/// Filter Excel file to keep only rows with keys and columns in the provided lists
pub(crate) fn filter_by_keys(
    source_file: &str,
    keys: &[String],
    key_column: &str,
    columns_to_keep: Option<&[String]>,
    output_file: &str,
) -> Result<(), FileStatus> {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let source_path = current_dir.join(source_file);

    let source_path_clone = source_path.clone();
    let source_data = match read_to_hash_map(source_path.clone(), key_column) {
        Ok(data) => {
            println!("Read {} Ok", source_path_clone.display());
            data
        }
        Err(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    };

    let headers = match read_headers(source_path) {
        Ok(headers) => headers,
        Err(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    };

    // Create a HashSet for faster lookup
    let key_set: HashSet<&String> = keys.iter().collect();

    // Filter the data to keep only rows with keys in the provided list
    let filtered_data: HashMap<String, HashMap<String, String>> = source_data
        .into_iter()
        .filter(|(key, _)| key_set.contains(key))
        .collect();

    println!(
        "Filtered {} rows from original data (input keys: {})",
        filtered_data.len(),
        keys.len()
    );

    // Filter headers if columns_to_keep is specified
    // Always keep 'key' and '备注' columns
    let filtered_headers = if let Some(cols) = columns_to_keep {
        let cols_set: HashSet<&String> = cols.iter().collect();
        headers
            .into_iter()
            .filter(|h| h == key_column || h == "备注" || cols_set.contains(h))
            .collect()
    } else {
        headers
    };

    println!("Output columns: {:?}", filtered_headers);

    match write_filtered_to_file(output_file, filtered_data, filtered_headers, key_column) {
        Ok(()) => {}
        Err(e) => {
            println!("Error occur: {:?}", e);
            return Err(FileStatus::VbaError(Error::Msg("Failed to write to file")));
        }
    }

    println!("Done.");
    Ok(())
}

fn read_to_hash_map(
    f: PathBuf,
    hash_key: &str,
) -> Result<HashMap<String, HashMap<String, String>>, FileStatus> {
    println!("Opening {:?}", f.display());
    let mut xl = open_workbook_auto(&f).unwrap();

    let mut data_store: HashMap<String, HashMap<String, String>> = HashMap::new();

    if let Some(sheet) = xl.sheet_names().first() {
        let range = xl.worksheet_range(sheet).expect("Cannot read sheet");

        let mut headers: Vec<String> = Vec::new();
        let hash_key_column = hash_key;

        for (row_index, row) in range.rows().enumerate() {
            if row_index == 0 {
                continue; // Skip the first row
            } else if row_index == 1 {
                headers = row.iter().map(|c| c.to_string()).collect(); // Second row as headers
            } else {
                let mut row_data: HashMap<String, String> = HashMap::new();
                let mut key_value = String::new();

                for (i, cell) in row.iter().enumerate() {
                    let header = &headers[i];
                    if header == hash_key_column {
                        key_value = cell.to_string();
                    } else {
                        row_data.insert(header.clone(), cell.to_string());
                    }
                }

                if !key_value.is_empty() {
                    data_store.insert(key_value, row_data);
                }
            }
        }
    }
    Ok(data_store)
}

fn read_headers(f: PathBuf) -> Result<Vec<String>, FileStatus> {
    let mut xl = open_workbook_auto(&f).unwrap();

    if let Some(sheet) = xl.sheet_names().first() {
        let range = xl.worksheet_range(sheet).expect("Cannot read sheet");

        for (row_index, row) in range.rows().enumerate() {
            if row_index == 1 {
                let headers = row.iter().map(|c| c.to_string()).collect();
                println!("Got headers {:?}", headers);
                return Ok(headers);
            }
        }
    }
    Err(FileStatus::RangeError(Error::Msg("No headers found")))
}

fn write_filtered_to_file(
    file: &str,
    data: HashMap<String, HashMap<String, String>>,
    headers: Vec<String>,
    key_column: &str,
) -> Result<(), XlsxError> {
    let header_note = "1、请上传小于 9999 条，99 MB的 EXCEL 文件。\n2、请在语言列增加对应的翻译，实现多语言的翻译配置。修改文案或清空文案都会覆盖原始数据，默认语言必须录入对应的翻译，否则会导致该行数据导入失败。新增加行数据将不会新增词条。\n3、请勿变更列数据的位置，请勿删除此行。";

    let workbook = Workbook::new(file)?;
    let sheet_name: Option<&str> = Some("全部");
    let mut sheet = workbook.add_worksheet(sheet_name)?;
    let lang_count = headers.len() as u16;

    // Header Note
    sheet.merge_range(
        0,
        0,
        0,
        lang_count - 1,
        header_note,
        Some(&Format::new().set_bold().set_text_wrap()),
    )?;
    sheet.set_row(0, 50.0, None)?;

    // Header
    for (i, header) in headers.iter().enumerate() {
        sheet.write_string(1, i as u16, header, None)?;
    }

    // Data
    let mut row_index = 2;
    for (key, row_data) in data.iter() {
        for (col_i, header) in headers.iter().enumerate() {
            let default_value = String::new();
            let value = if header == key_column {
                key
            } else {
                row_data.get(header).unwrap_or(&default_value)
            };
            sheet.write_string(row_index, col_i as u16, value, None)?;
        }
        row_index += 1;
    }

    workbook.close()?;
    Ok(())
}
