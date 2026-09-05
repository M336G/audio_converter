use actix_cors::Cors;
use actix_files;
use actix_web::middleware::Compress;
use actix_web::{middleware, options, web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use essi_ffmpeg::FFmpeg;
use tokio::sync::Semaphore;
use std::{env, fs};
use std::io;

mod convert;

#[actix_rt::main]
async fn main() -> io::Result<()> {
    dotenv().ok();
    let server_port: u16 = env::var("SERVER_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(5555);

    let max_file_size: usize = env::var("MAX_FILE_SIZE")
        .ok()
        .and_then(|max_size| max_size.parse().ok())
        .unwrap_or(20);

    let max_concurrent_conversions: usize = env::var("MAX_CONCURRENT_CONVERSIONS")
        .ok()
        .and_then(|max_conversions| max_conversions.parse().ok())
        .unwrap_or(10);

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
    println!("Maximum file size limit set to {} MB", max_file_size);
    println!("Maximum concurrent connections set to {}", max_concurrent_conversions);

    let conversion_path = std::env::temp_dir().join("audio-converter");
    if conversion_path.exists() {
        if let Err(error) = fs::remove_dir_all(&conversion_path) {
            eprintln!("Failed to cleanup conversions directory: {}", error);
        }
    }
    if let Err(error) = fs::create_dir_all(&conversion_path) {
        eprintln!("Failed to create conversions directory: {}", error);
    }

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["OPTIONS", "GET", "POST"])
            .allow_any_header();

        App::new()
            .app_data(web::PayloadConfig::new(max_file_size * 1024 * 1024))
            .app_data(web::Data::new(Semaphore::new(max_concurrent_conversions)))
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