use actix_rt::time::timeout;
use actix_web::{post, web, HttpResponse, Responder};
use essi_ffmpeg::FFmpeg;
use randomizer::Randomizer;
use serde::Deserialize;
use tokio::sync::Semaphore;
use std::fs::{self, File};
use std::io::Write;
use std::time::Duration;

#[derive(Deserialize)]
pub struct ConvertParams {
    format: Option<String>
}

const SUPPORTED_FORMATS: [&'static str; 7] = [
    "mp3", "ogg", "opus", "wav", "webm", "flac", "aac"
];

fn format_to_mime(format: &str) -> &'static str {
    match format {
        "mp3" => "audio/mpeg",
        "opus" => "audio/ogg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "webm" => "audio/webm",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}

#[post("/")]
pub async fn convert(body: web::Bytes, query: web::Query<ConvertParams>, limiter: web::Data<Semaphore>) -> impl Responder {
    let _permit = limiter.acquire().await.unwrap();

    let format = query.format.clone().unwrap_or_else(|| "mp3".to_string());

    if !SUPPORTED_FORMATS.contains(&format.as_str()) {
        return HttpResponse::UnsupportedMediaType()
            .content_type("text/plain")
            .body("Unsupported output format!");
    }

    if body.is_empty() {
        return HttpResponse::BadRequest()
        .content_type("text/plain")
        .body("Empty file!");
    }

    // Detect actual file type using infer
    let file_type = infer::get(&body);
    let input_extension = match file_type {
        Some(ft) => {
            let ext = ft.extension();
            let mime = ft.mime_type();

            if ext == format {
                return HttpResponse::BadRequest()
                    .content_type("text/plain")
                    .body("You're trying to convert a file to the same format!");
            }

            if !SUPPORTED_FORMATS.contains(&ext) {
                return HttpResponse::UnsupportedMediaType()
                    .content_type("text/plain")
                    .body(format!("Unsupported input file type: {} ({})", ext, mime));
            }
            ext
        }
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body("Could not detect input file type")
        }
    };

    let conversion_path = std::env::temp_dir().join("audio-converter");
    let conversion_id = Randomizer::ALPHANUMERIC(32).string().unwrap();
    let input_path = conversion_path.join(format!("{}-input.{}", conversion_id, input_extension));
    let output_path = conversion_path.join(format!("{}-output.{}", conversion_id, format));

    println!("New conversion request:");
    println!("- ID: {}", conversion_id);
    println!("- Size: {:.2} MB", body.len() as f32 / 1024.0 / 1024.0);
    println!("- From: {}", input_extension);
    println!("- To: {}", format);

    // Create the input file and write to it
    let result = async {
        let mut file = File::create(&input_path).map_err(|error| {
            eprintln!("Failed creating file: {}", error);
            HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Internal Server Error")
        })?;

        file.write_all(&body).map_err(|error| {
            eprintln!("Failed writing to file: {}", error);
            HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Internal Server Error")
        })?;

        println!("Converting {} from {} to {}...", conversion_id, input_extension, format);

        // Convert the file
        let mut ffmpeg = {
            let mut cmd = FFmpeg::new()
                .stderr(std::process::Stdio::null())
                .input_with_file(input_path.clone().into())
                .done();

            match format.as_str() {
                "aac" => {
                    cmd = cmd.arg("-c:a").arg("aac");
                }
                "ogg" => {
                    cmd = cmd.arg("-c:a").arg("libvorbis");
                }
                "opus" => {
                    cmd = cmd.arg("-c:a").arg("libopus");
                }
                _ => {}
            }

            match cmd.output_as_file(output_path.clone().into()).done().start() {
                Ok(proc) => proc,
                Err(error) => {
                    eprintln!("Failed to start FFmpeg: {}", error);
                    return Err(HttpResponse::InternalServerError()
                        .content_type("text/plain")
                        .body("Internal Server Error"));
                }
            }
        };

        match timeout(Duration::from_secs(120), web::block(move || ffmpeg.wait())).await {
            Ok(Ok(Ok(_status))) => {}
            Ok(Ok(Err(error))) => {
                eprintln!("FFmpeg exited with error: {}", error);
                return Err(HttpResponse::InternalServerError().body("Internal Server Error"));
            }
            Ok(Err(_)) => {
                return Err(HttpResponse::InternalServerError().body("Internal Server Error"));
            }
            Err(_) => {
                eprintln!("FFmpeg conversion {} timed out", conversion_id);
                return Err(HttpResponse::InternalServerError().body("Internal Server Error"));
            }
        }

        let read_path = output_path.clone();
        let output_data = web::block(move || fs::read(&read_path))
            .await
            .map_err(|_| HttpResponse::InternalServerError().body("Internal Server Error"))?
            .map_err(|error| {
                eprintln!("Failed to read file: {}", error);
                HttpResponse::InternalServerError()
                    .content_type("text/plain")
                    .body("Internal Server Error")
            })?;

        println!("Successfully converted {} from {} to {}!", conversion_id, input_extension, format);

        Ok(HttpResponse::Ok()
            .content_type(format_to_mime(&format))
            .insert_header(("Access-Control-Allow-Origin", "*"))
            .body(output_data))
    }.await;

    // Cleanup -input and -output
    for path in [&input_path, &output_path] {
        if let Err(error) = fs::remove_file(path) {
            eprintln!("Failed to delete file {:#?}: {}", path, error);
        }
    }

    result.unwrap_or_else(|resp| resp)
}
