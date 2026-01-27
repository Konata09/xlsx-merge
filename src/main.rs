use actix_files::Files;
use actix_multipart::form::{
    tempfile::{TempFile, TempFileConfig},
    text::Text,
    MultipartForm,
};
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use mime_guess::from_path;
use rust_embed::Embed;
use serde_derive::Serialize;

mod filter_by_keys;
mod merge;

#[derive(Embed)]
#[folder = "public/"]
struct Asset;

fn handle_embedded_file(path: &str) -> HttpResponse {
    match Asset::get(path) {
        Some(content) => HttpResponse::Ok()
            .content_type(from_path(path).first_or_octet_stream().as_ref())
            .body(content.data.into_owned()),
        None => HttpResponse::NotFound().body("404 Not Found"),
    }
}

async fn index() -> Result<impl Responder, Error> {
    Ok(handle_embedded_file("index.html"))
}

#[derive(Serialize)]
struct Response<T> {
    data: T,
    ret: i16,
    msg: String,
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    source_file: TempFile,
    ref_file: TempFile,
    ref_column: Text<String>,
    fill_columns: Text<String>,
}

#[derive(Debug, MultipartForm)]
struct FilterForm {
    source_file: TempFile,
    keys: Text<String>,
    columns: Text<String>,
}

#[derive(Debug, MultipartForm)]
struct HeadersForm {
    source_file: TempFile,
}

async fn handle_merge_post(
    MultipartForm(form): MultipartForm<UploadForm>,
) -> Result<impl Responder, Error> {
    let output_file;
    let source_file;
    let ref_file;

    if form.source_file.size > 0 {
        if let Some(file_name) = form.source_file.file_name {
            let path = format!("/tmp/xlsx_merge/upload/{}", file_name);
            source_file = path.clone();
            form.source_file.file.persist(path).unwrap();
            output_file = format!("/tmp/xlsx_merge/output/{}_merge.xlsx", file_name);
        } else {
            return Ok(HttpResponse::BadRequest().body("Source file name is missing"));
        }
    } else {
        return Ok(HttpResponse::BadRequest().body("Source file size is zero"));
    }

    if form.ref_file.size > 0 {
        if let Some(file_name) = form.ref_file.file_name {
            let path = format!("/tmp/{}", file_name);
            ref_file = path.clone();
            form.ref_file.file.persist(path).unwrap();
        } else {
            return Ok(HttpResponse::BadRequest().body("Reference file name is missing"));
        }
    } else {
        return Ok(HttpResponse::BadRequest().body("Reference file size is zero"));
    }

    if form.ref_column.len() == 0 {
        return Ok(HttpResponse::BadRequest().body("Reference Column is missing"));
    }

    if form.fill_columns.len() == 0 {
        return Ok(HttpResponse::BadRequest().body("To be Filled Columns is missing"));
    } else {
        let mut fill_columns: Vec<&str> = form.fill_columns.split('|').collect();
        fill_columns.retain(|&s| !s.is_empty());
        if fill_columns.is_empty() {
            return Ok(HttpResponse::BadRequest().body("To be Filled Columns is invalid"));
        }
        if let Ok(()) = merge::merge(
            &source_file,
            &ref_file,
            form.ref_column.as_str(),
            fill_columns.as_slice(),
            &output_file,
        ) {
            let components: Vec<&str> = output_file.split('/').collect();
            if let Some(filename) = components.iter().last() {
                Ok(HttpResponse::Ok().json(Response {
                    data: format!("/output/{}", filename),
                    ret: 0,
                    msg: String::new(),
                }))
            } else {
                Ok(HttpResponse::Ok().json(Response {
                    data: (),
                    ret: -1,
                    msg: "Error when parsing output file".to_string(),
                }))
            }
        } else {
            Ok(HttpResponse::Ok().json(Response {
                data: (),
                ret: -1,
                msg: "Error when merge files".to_string(),
            }))
        }
    }
}

async fn handle_get_headers(
    MultipartForm(form): MultipartForm<HeadersForm>,
) -> Result<impl Responder, Error> {
    let source_file;

    if form.source_file.size > 0 {
        if let Some(file_name) = form.source_file.file_name {
            let path = format!("/tmp/xlsx_merge/upload/{}", file_name);
            source_file = path.clone();
            form.source_file.file.persist(path).unwrap();
        } else {
            return Ok(HttpResponse::BadRequest().body("Source file name is missing"));
        }
    } else {
        return Ok(HttpResponse::BadRequest().body("Source file size is zero"));
    }

    match filter_by_keys::get_headers(&source_file) {
        Ok(headers) => {
            // Filter out 'key' and '备注' columns from the list
            let language_columns: Vec<String> = headers
                .into_iter()
                .filter(|h| h != "key" && h != "备注")
                .collect();
            Ok(HttpResponse::Ok().json(Response {
                data: language_columns,
                ret: 0,
                msg: String::new(),
            }))
        }
        Err(_) => Ok(HttpResponse::Ok().json(Response {
            data: Vec::<String>::new(),
            ret: -1,
            msg: "Error reading headers from file".to_string(),
        })),
    }
}

async fn handle_filter_post(
    MultipartForm(form): MultipartForm<FilterForm>,
) -> Result<impl Responder, Error> {
    let output_file;
    let source_file;

    if form.source_file.size > 0 {
        if let Some(file_name) = form.source_file.file_name {
            let path = format!("/tmp/xlsx_merge/upload/{}", file_name);
            source_file = path.clone();
            form.source_file.file.persist(path).unwrap();
            output_file = format!("/tmp/xlsx_merge/output/{}_filtered.xlsx", file_name);
        } else {
            return Ok(HttpResponse::BadRequest().body("Source file name is missing"));
        }
    } else {
        return Ok(HttpResponse::BadRequest().body("Source file size is zero"));
    }

    if form.keys.len() == 0 {
        return Ok(HttpResponse::BadRequest().body("Keys are missing"));
    }

    // Parse keys from the textarea (one key per line)
    let keys: Vec<String> = form
        .keys
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if keys.is_empty() {
        return Ok(HttpResponse::BadRequest().body("No valid keys provided"));
    }

    // Parse columns to keep (pipe-separated)
    let columns_to_keep: Option<Vec<String>> = if form.columns.len() > 0 {
        let cols: Vec<String> = form
            .columns
            .split('|')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if cols.is_empty() {
            None
        } else {
            Some(cols)
        }
    } else {
        None
    };

    if let Ok(()) = filter_by_keys::filter_by_keys(
        &source_file,
        &keys,
        "key",
        columns_to_keep.as_deref(),
        &output_file,
    ) {
        let components: Vec<&str> = output_file.split('/').collect();
        if let Some(filename) = components.iter().last() {
            Ok(HttpResponse::Ok().json(Response {
                data: format!("/output/{}", filename),
                ret: 0,
                msg: String::new(),
            }))
        } else {
            Ok(HttpResponse::Ok().json(Response {
                data: (),
                ret: -1,
                msg: "Error when parsing output file".to_string(),
            }))
        }
    } else {
        Ok(HttpResponse::Ok().json(Response {
            data: (),
            ret: -1,
            msg: "Error when filtering file".to_string(),
        }))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    log::info!("creating temporary upload directory");
    std::fs::create_dir_all("/tmp/xlsx_merge/upload")?;
    std::fs::create_dir_all("/tmp/xlsx_merge/output")?;

    log::info!("starting HTTP server at http://0.0.0.0:8080");

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(TempFileConfig::default().directory("/tmp/xlsx_merge"))
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/merge").route(web::post().to(handle_merge_post)))
            .service(web::resource("/headers").route(web::post().to(handle_get_headers)))
            .service(web::resource("/filter").route(web::post().to(handle_filter_post)))
            .service(Files::new("/output", "/tmp/xlsx_merge/output/"))
    })
    .bind(("0.0.0.0", 8080))?
    .workers(2)
    .run()
    .await
}
