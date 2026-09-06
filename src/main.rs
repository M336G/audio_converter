use actix_cors::Cors;
use actix_files;
use actix_web::middleware::Compress;
use actix_web::{middleware, options, web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use essi_ffmpeg::FFmpeg;
use tokio::sync::Semaphore;
use std::time::{Duration, SystemTime};
use std::{env, fs};
use std::io;

mod convert;

pub struct UploadLimiter(Semaphore);
pub struct ConversionLimiter(Semaphore);

#[actix_rt::main]
async fn main() -> io::Result<()> {
    dotenv().ok();
    let server_port: u16 = env::var("SERVER_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(5555);

    let max_file_size: usize = env::var("MAX_FILE_SIZE")
        .ok()
        .and_then(|max_size_mb| max_size_mb.parse::<usize>().ok())
        .and_then(|max_size_mb| max_size_mb.checked_mul(1024 * 1024))
        .unwrap_or(20 * 1024 * 1024);

    let max_concurrent_uploads: usize = env::var("MAX_CONCURRENT_UPLOADS")
        .ok()
        .and_then(|max_uploads| max_uploads.parse().ok())
        .unwrap_or(30);

    let max_concurrent_conversions: usize = env::var("MAX_CONCURRENT_CONVERSIONS")
        .ok()
        .and_then(|max_conversions| max_conversions.parse().ok())
        .unwrap_or(10);

    let allowed_origins: Option<Vec<String>> = env::var("ALLOWED_ORIGINS")
        .ok()
        .map(|origins| origins.split(',').map(|origin| origin.trim().to_string()).collect());

    if let Some((handle, mut progress)) = FFmpeg::auto_download().await.unwrap() {
        let progress_task = actix_rt::spawn(async move {
            println!("Downloading FFmpeg...");
            while progress.recv().await.is_some() {}
            println!("Downloaded FFmpeg!");
        });

        handle.await.unwrap().unwrap();
        progress_task.await.unwrap();
    }

    println!("Server started on http://0.0.0.0:{}/!", server_port);
    println!("Maximum file size limit set to {} MB", max_file_size / 1024 / 1024);
    println!("Maximum concurrent uploads set to {}", max_concurrent_uploads);
    println!("Maximum concurrent conversions set to {}", max_concurrent_conversions);
    if let Some(origins) = &allowed_origins {
        println!("Restricted allowed origins CORS header to {:?}", origins);
    } else {
        println!("Current allowed origins CORS header allows any origin to request");
    }

    let conversion_path = std::env::temp_dir().join("audio-converter");
    if let Err(error) = fs::create_dir_all(&conversion_path) {
        eprintln!("Failed to create conversions directory: {}", error);
    }

    let conversion_path_for_cleanup = conversion_path.clone();
    actix_rt::spawn(async move {
        loop {
            actix_rt::time::sleep(Duration::from_mins(5)).await;

            let entries = match fs::read_dir(&conversion_path_for_cleanup) {
                Ok(entries) => entries,
                Err(error) => {
                    eprintln!("Failed to read conversions directory: {}", error);
                    continue;
                }
            };

            let mut cleaned_up = 0;
            for entry in entries.flatten() {
                let path = entry.path();

                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => continue
                };

                let modified = match metadata.modified() {
                    Ok(modified) => modified,
                    Err(_) => continue
                };

                let age = match SystemTime::now().duration_since(modified) {
                    Ok(age) => age,
                    Err(_) => continue
                };

                if age > Duration::from_mins(30) {
                    match fs::remove_file(&path) {
                        Ok(_) => cleaned_up += 1,
                        Err(error) => eprintln!("Failed cleaning up {:#?}: {}", path, error),
                    }
                }
            }

            if cleaned_up > 0 {
                println!("Cleaned up {} stale file(s)!", cleaned_up);
            }
        }
    });

    let conversion_path_for_server = web::Data::new(conversion_path.clone());
    let max_file_size_for_server = web::Data::new(max_file_size);
    let upload_limiter_for_server = web::Data::new(UploadLimiter(Semaphore::new(max_concurrent_uploads)));
    let conversion_limiter_for_server = web::Data::new(ConversionLimiter(Semaphore::new(max_concurrent_conversions)));

    HttpServer::new(move || {
        let cors = match &allowed_origins {
            Some(origins) if !origins.is_empty() => {
                let mut cors = Cors::default()
                    .allowed_methods(vec!["OPTIONS", "GET", "POST"])
                    .allow_any_header();
                for origin in origins {
                    cors = cors.allowed_origin(origin);
                }
                cors
            }
            _ => Cors::default()
                .allow_any_origin()
                .allowed_methods(vec!["OPTIONS", "GET", "POST"])
                .allow_any_header(),
        };

        App::new()
            .app_data(conversion_path_for_server.clone())
            .app_data(max_file_size_for_server.clone())
            .app_data(upload_limiter_for_server.clone())
            .app_data(conversion_limiter_for_server.clone())
            .wrap(middleware::Logger::default())
            .wrap(Compress::default())
            .wrap(cors)
            .service(options)
            .service(convert::convert)
            .default_service(actix_files::Files::new("/", "./public").index_file("index.html"))
    })
    .bind(format!("0.0.0.0:{}", server_port))?
    .run()
    .await
}

#[options("/")]
async fn options() -> impl Responder {
    HttpResponse::NoContent()
}