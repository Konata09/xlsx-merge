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
            .service(Files::new("/output", "/tmp/xlsx_merge/output/"))
    })
    .bind(("0.0.0.0", 8080))?
    .workers(2)
    .run()
    .await
}
