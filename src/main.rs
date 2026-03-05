use actix_files::Files;
use actix_multipart::form::{
    tempfile::{TempFile, TempFileConfig},
    text::Text,
    MultipartForm,
};
use actix_web::rt;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use mime_guess::from_path;
use rust_embed::Embed;
use serde::Deserialize;
use serde_derive::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

mod ai_translate;
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

#[derive(Debug, MultipartForm)]
struct AiTranslateForm {
    source_file: TempFile,
    source_lang: Text<String>,
    target_lang: Text<String>,
}

#[derive(Debug, Clone)]
enum TranslateJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl TranslateJobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
struct TranslateJob {
    status: TranslateJobStatus,
    total: usize,
    processed: usize,
    translated: usize,
    skipped: usize,
    download_url: Option<String>,
    message: String,
}

struct AppState {
    jobs: Mutex<HashMap<String, TranslateJob>>,
    next_job_id: AtomicU64,
}

#[derive(Serialize)]
struct AiTranslateStartData {
    job_id: String,
}

#[derive(Serialize)]
struct AiTranslateProgressData {
    job_id: String,
    status: String,
    total: usize,
    processed: usize,
    translated: usize,
    skipped: usize,
    download_url: Option<String>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct AiTranslateProgressPath {
    job_id: String,
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

async fn handle_ai_translate_post(
    app_state: web::Data<AppState>,
    MultipartForm(form): MultipartForm<AiTranslateForm>,
) -> Result<impl Responder, Error> {
    let source_file;
    let output_file;
    let download_url;
    let job_index = app_state.next_job_id.fetch_add(1, Ordering::Relaxed) + 1;
    let job_id = format!("translate-{}", job_index);

    if form.source_file.size > 0 {
        if let Some(file_name) = form.source_file.file_name {
            let path = format!("/tmp/xlsx_merge/upload/{}_{}", job_id, file_name);
            source_file = path.clone();
            form.source_file.file.persist(path).unwrap();
            let output_name = format!("{}_{}_ai_translated.xlsx", job_id, file_name);
            output_file = format!("/tmp/xlsx_merge/output/{}", output_name);
            download_url = format!("/output/{}", output_name);
        } else {
            return Ok(HttpResponse::BadRequest().body("Source file name is missing"));
        }
    } else {
        return Ok(HttpResponse::BadRequest().body("Source file size is zero"));
    }

    let source_lang = form.source_lang.as_str().trim().to_string();
    let target_lang = form.target_lang.as_str().trim().to_string();

    if source_lang.is_empty() {
        return Ok(HttpResponse::BadRequest().body("Source language is missing"));
    }
    if target_lang.is_empty() {
        return Ok(HttpResponse::BadRequest().body("Target language is missing"));
    }
    if source_lang == target_lang {
        return Ok(
            HttpResponse::BadRequest().body("Source and target languages cannot be the same")
        );
    }

    {
        let mut jobs = app_state.jobs.lock().unwrap();
        jobs.insert(
            job_id.clone(),
            TranslateJob {
                status: TranslateJobStatus::Queued,
                total: 0,
                processed: 0,
                translated: 0,
                skipped: 0,
                download_url: None,
                message: "Job queued".to_string(),
            },
        );
    }

    let app_state_clone = app_state.clone();
    let job_id_clone = job_id.clone();
    let source_file_clone = source_file.clone();
    let source_lang_clone = source_lang.clone();
    let target_lang_clone = target_lang.clone();
    let output_file_clone = output_file.clone();
    let download_url_clone = download_url.clone();

    rt::spawn(async move {
        {
            let mut jobs = app_state_clone.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(&job_id_clone) {
                job.status = TranslateJobStatus::Running;
                job.message = "Starting AI translation".to_string();
            }
        }

        let progress_state = app_state_clone.clone();
        let progress_job_id = job_id_clone.clone();
        let result = ai_translate::ai_translate_with_progress(
            &source_file_clone,
            &source_lang_clone,
            &target_lang_clone,
            &output_file_clone,
            move |progress| {
                let mut jobs = progress_state.jobs.lock().unwrap();
                if let Some(job) = jobs.get_mut(&progress_job_id) {
                    job.status = TranslateJobStatus::Running;
                    job.total = progress.total;
                    job.processed = progress.processed;
                    job.translated = progress.translated;
                    job.skipped = progress.skipped;
                    job.message = format!("Processing {} / {}", progress.processed, progress.total);
                }
            },
        )
        .await;

        let mut jobs = app_state_clone.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&job_id_clone) {
            match result {
                Ok(translate_result) => {
                    job.status = TranslateJobStatus::Completed;
                    job.processed = job.total;
                    job.translated = translate_result.translated_count;
                    job.skipped = translate_result.skipped_count;
                    job.download_url = Some(download_url_clone.clone());
                    job.message = format!(
                        "Translated {} entries, skipped {} entries",
                        translate_result.translated_count, translate_result.skipped_count
                    );
                }
                Err(error_message) => {
                    job.status = TranslateJobStatus::Failed;
                    job.message = error_message;
                }
            }
        }
    });

    Ok(HttpResponse::Ok().json(Response {
        data: AiTranslateStartData { job_id },
        ret: 0,
        msg: "AI translation job started".to_string(),
    }))
}

async fn handle_ai_translate_progress(
    app_state: web::Data<AppState>,
    path: web::Path<AiTranslateProgressPath>,
) -> Result<impl Responder, Error> {
    let job_id = path.into_inner().job_id;
    let jobs = app_state.jobs.lock().unwrap();

    if let Some(job) = jobs.get(&job_id) {
        return Ok(HttpResponse::Ok().json(Response {
            data: AiTranslateProgressData {
                job_id,
                status: job.status.as_str().to_string(),
                total: job.total,
                processed: job.processed,
                translated: job.translated,
                skipped: job.skipped,
                download_url: job.download_url.clone(),
                message: job.message.clone(),
            },
            ret: 0,
            msg: String::new(),
        }));
    }

    Ok(HttpResponse::Ok().json(Response {
        data: (),
        ret: -1,
        msg: "AI translate job not found".to_string(),
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info,xlsx_merge=debug"));

    log::info!("creating temporary upload directory");
    std::fs::create_dir_all("/tmp/xlsx_merge/upload")?;
    std::fs::create_dir_all("/tmp/xlsx_merge/output")?;

    log::info!("starting HTTP server at http://0.0.0.0:8080");

    let app_state = web::Data::new(AppState {
        jobs: Mutex::new(HashMap::new()),
        next_job_id: AtomicU64::new(0),
    });

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(app_state.clone())
            .app_data(TempFileConfig::default().directory("/tmp/xlsx_merge"))
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/merge").route(web::post().to(handle_merge_post)))
            .service(web::resource("/headers").route(web::post().to(handle_get_headers)))
            .service(web::resource("/filter").route(web::post().to(handle_filter_post)))
            .service(web::resource("/ai-translate").route(web::post().to(handle_ai_translate_post)))
            .service(
                web::resource("/ai-translate/progress/{job_id}")
                    .route(web::get().to(handle_ai_translate_progress)),
            )
            .service(Files::new("/output", "/tmp/xlsx_merge/output/"))
    })
    .bind(("0.0.0.0", 8080))?
    .workers(2)
    .run()
    .await
}
