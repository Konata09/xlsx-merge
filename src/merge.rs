use std::collections::HashMap;
use std::env;
use std::path::{PathBuf};
use calamine::{open_workbook_auto, Error, Reader};
use glob::{GlobError};
use xlsxwriter::{Format, Workbook, XlsxError};

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FileStatus {
    VbaError(Error),
    RangeError(Error),
    Glob(GlobError),
}

pub(crate) fn merge(source_file: &str, ref_file: &str, column: &str, output_file: &str) -> Result<(), FileStatus> {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let from_file = current_dir.join(ref_file);
    let to_file = current_dir.join(source_file);

    let from_file_clone = from_file.clone();
    let from_data = match read_to_hash_map(from_file) {
        Ok(data) => {
            println!("Read {} Ok", from_file_clone.display());
            data
        }
        Err(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    };

    let to_file_clone = to_file.clone();
    let to_data = match read_to_hash_map(to_file.clone()) {
        Ok(data) => {
            println!("Read {:?} Ok", to_file_clone.display());
            data
        }
        Err(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    };

    let headers = match read_headers(to_file) {
        Ok(headers) => headers,
        Err(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    };

    let target_file = output_file;

    match write_to_file(target_file, from_data, to_data, column, headers) {
        Ok(()) => {}
        Err(e) => {
            println!("Error occur: {:?}", e);
            return Err(FileStatus::VbaError(Error::Msg("Failed to write to file")));
        }
    }

    println!("Done.");
    Ok(())
}

fn write_to_file(file: &str, from: HashMap<String, HashMap<String, String>>, to: HashMap<String, HashMap<String, String>>, update_column: &str, headers: Vec<String>) -> Result<(), XlsxError> {
    let header_note = "1、请上传小于 9999 条，99 MB的 EXCEL 文件。\n2、请在语言列增加对应的翻译，实现多语言的翻译配置。修改文案或清空文案都会覆盖原始数据，默认语言必须录入对应的翻译，否则会导致该行数据导入失败。新增加行数据将不会新增词条。\n3、请勿变更列数据的位置，请勿删除此行。";

    let workbook = Workbook::new(file)?;
    let sheet_name: Option<&str> = Some("全部");
    let mut sheet = workbook.add_worksheet(sheet_name)?;
    let lang_count = headers.len() as u16;

    // Header Note
    sheet.merge_range(0, 0, 0, lang_count - 1, header_note, Some(
        &Format::new().set_bold().set_text_wrap()
    ))?;
    sheet.set_row(0, 50.0, None)?;

    // Header
    for (i, header) in headers.iter().enumerate() {
        sheet.write_string(1, i as u16, header, None)?;
    }

    // Data
    let mut row_index = 2;
    for (key, row_data) in to.iter() {
        for (col_i, header) in headers.iter().enumerate() {
            let value;
            let default_value = String::new();
            if header == update_column {
                match from.get(key) {
                    None => {
                        value = &default_value;
                    }
                    Some(to_row) => {
                        value = to_row.get(header).unwrap_or(&default_value);
                    }
                }
            } else if header == "key" {
                value = key;
            } else {
                value = row_data.get(header).unwrap_or(&default_value);
            }
            sheet.write_string(row_index, col_i as u16, value, None)?;
        }
        row_index += 1;
    }

    workbook.close()?;
    Ok(())
}

fn read_to_hash_map(f: PathBuf) -> Result<HashMap<String, HashMap<String, String>>, FileStatus> {
    println!("Opening {:?}", f.display());
    let mut xl = open_workbook_auto(&f).unwrap();

    let mut data_store: HashMap<String, HashMap<String, String>> = HashMap::new();

    if let Some(sheet) = xl.sheet_names().first() {
        let range = xl.worksheet_range(sheet).expect("Cannot read sheet");

        let mut headers: Vec<String> = Vec::new();
        let key_column = "key"; // Replace with the actual key column name

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
                    if header == key_column {
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
