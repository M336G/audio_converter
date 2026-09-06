use actix_multipart::Multipart;
use actix_rt::time::timeout;
use actix_web::{post, web, HttpResponse, Responder};
use essi_ffmpeg::FFmpeg;
use futures_util::{StreamExt, TryStreamExt};
use randomizer::Randomizer;
use tokio_util::io::ReaderStream;
use std::fs::{self, File};
use std::io::Write;
use std::time::Duration;
use tokio::sync::Semaphore;

const SUPPORTED_FORMATS: [&'static str; 9] = [
    "mp3", "ogg", "opus", "wav", "weba", "flac", "aac", "m4a", "aiff"
];

fn format_to_mime(format: &str) -> &'static str {
    match format {
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "weba" => "audio/webm",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "aiff" => "audio/aiff",
        _ => "application/octet-stream"
    }
}

#[post("/")]
pub async fn convert(mut payload: Multipart, limiter: web::Data<Semaphore>, max_file_size: web::Data<usize>) -> impl Responder {
    let max_file_size = **max_file_size;

    let conversion_path = std::env::temp_dir().join("audio-converter");
    let conversion_id = Randomizer::ALPHANUMERIC(32).string().unwrap();
    let tmp_input_path = conversion_path.join(format!("{}-input.tmp", conversion_id));

    let mut format: Option<String> = None;
    let mut received_file = false;
    let mut infer_buffer: Vec<u8> = Vec::with_capacity(8192);
    let mut total_size: usize = 0;

    // Go through every field in the multipart form
    let read_result = timeout(Duration::from_secs(90), async {
        while let Ok(Some(mut field)) = payload.try_next().await {
            let field_name = field.name().unwrap_or_default().to_string();

            match field_name.as_str() {
                // Get the conversion format
                "format" => {
                    let mut value = Vec::with_capacity(16);
                    while let Some(chunk) = field.next().await {
                        match chunk {
                            Ok(bytes) => {
                                if value.len() + bytes.len() > 16 {
                                    return Some(HttpResponse::BadRequest()
                                        .content_type("text/plain")
                                        .body("Invalid form data"));
                                }
                                value.extend_from_slice(&bytes);
                            }
                            Err(_) => {
                                return Some(HttpResponse::BadRequest()
                                    .content_type("text/plain")
                                    .body("Invalid form data"));
                            }
                        }
                    }
                    format = String::from_utf8(value).ok();
                }

                // Get the file to convert
                "file" => {
                    if received_file {
                        while field.next().await.is_some() {}
                        continue;
                    }
                    received_file = true;

                    let mut file = match File::create(&tmp_input_path) {
                        Ok(file) => file,
                        Err(error) => {
                            eprintln!("Failed creating file: {}", error);
                            return Some(HttpResponse::InternalServerError()
                                .content_type("text/plain")
                                .body("Internal Server Error")
                            );
                        }
                    };

                    // Stream the file to the disk (under a temporary name set earlier)
                    while let Some(chunk) = field.next().await {
                        let chunk = match chunk {
                            Ok(bytes) => bytes,
                            Err(_) => {
                                let _ = fs::remove_file(&tmp_input_path);
                                return Some(HttpResponse::BadRequest()
                                    .content_type("text/plain")
                                    .body("Invalid form data")
                                );
                            }
                        };

                        total_size += chunk.len();
                        if total_size > max_file_size {
                            let _ = fs::remove_file(&tmp_input_path);
                            return Some(HttpResponse::PayloadTooLarge()
                                .content_type("text/plain")
                                .body("File too large!")
                            );
                        }

                        // Store the first 8kb of the file in memory for infer
                        // to detect the file type
                        if infer_buffer.len() < 8192 {
                            let take = (8192 - infer_buffer.len()).min(chunk.len());
                            infer_buffer.extend_from_slice(&chunk[..take]);
                        }

                        if let Err(error) = file.write_all(&chunk) {
                            eprintln!("Failed writing to file: {}", error);

                            let _ = fs::remove_file(&tmp_input_path);
                            return Some(HttpResponse::InternalServerError()
                                .content_type("text/plain")
                                .body("Internal Server Error")
                            );
                        }
                    }
                }

                // Ignore other fields
                _ => {
                    while field.next().await.is_some() {}
                }
            }
        }
        None
    }).await;

    match read_result {
        Ok(Some(early_response)) => {
            let _ = fs::remove_file(&tmp_input_path);
            return early_response;
        }
        Ok(None) => {}
        Err(_) => {
            let _ = fs::remove_file(&tmp_input_path);
            return HttpResponse::RequestTimeout()
                .content_type("text/plain")
                .body("Took too long to upload!");
        }
    }

    if !received_file || total_size == 0 {
        let _ = fs::remove_file(&tmp_input_path);
        return HttpResponse::BadRequest()
            .content_type("text/plain")
            .body("Empty file!");
    }

    let format = match format {
        Some(format) if SUPPORTED_FORMATS.contains(&format.as_str()) => format,
        Some(_) => {
            let _ = fs::remove_file(&tmp_input_path);
            return HttpResponse::UnsupportedMediaType()
                .content_type("text/plain")
                .body("Unsupported output format!");
        }
        None => {
            let _ = fs::remove_file(&tmp_input_path);
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body("Missing \"format\" field!");
        }
    };

    let input_extension = match infer::get(&infer_buffer) {
        Some(ft) => {
            let mut ext = ft.extension();
            let mime = ft.mime_type();

            if ext == "webm" && mime == "video/webm" {
                ext = "weba";
            }

            if ext == format {
                let _ = fs::remove_file(&tmp_input_path);
                return HttpResponse::BadRequest()
                    .content_type("text/plain")
                    .body("You're trying to convert a file to the same format!");
            }

            if !SUPPORTED_FORMATS.contains(&ext) {
                let _ = fs::remove_file(&tmp_input_path);
                return HttpResponse::UnsupportedMediaType()
                    .content_type("text/plain")
                    .body(format!("Unsupported input file type: {} ({})", ext, mime));
            }
            ext
        }
        None => {
            let _ = fs::remove_file(&tmp_input_path);
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body("Could not detect input file type");
        }
    };

    let input_path = conversion_path.join(format!("{}-input.{}", conversion_id, input_extension));
    let output_path = conversion_path.join(format!("{}-output.{}", conversion_id, format));

    if let Err(error) = fs::rename(&tmp_input_path, &input_path) {
        eprintln!("Failed to rename input file: {}", error);
        let _ = fs::remove_file(&tmp_input_path);
        return HttpResponse::InternalServerError()
            .content_type("text/plain")
            .body("Internal Server Error");
    }

    println!("New conversion request:");
    println!("- ID: {}", conversion_id);
    println!("- Size: {:.2} MB", total_size as f32 / 1024.0 / 1024.0);
    println!("- From: {}", input_extension);
    println!("- To: {}", format);

    println!("Converting {} from {} to {}...", conversion_id, input_extension, format);

    let result = async {
        // If the amount of conversions reached MAX_CONCURRENT_CONVERSIONS then this will lock the request until one of them is done
        let _permit = limiter.acquire().await.unwrap();

        // Convert the file
        let mut ffmpeg = {
            let mut cmd = FFmpeg::new()
                .stderr(std::process::Stdio::null())
                .input_with_file(input_path.clone().into())
                .done();

            cmd = cmd
                .arg("-map")
                .arg("0:a:0")
                .arg("-vn");

            match format.as_str() {
                "mp3" => {
                    cmd = cmd.arg("-c:a").arg("libmp3lame");
                }
                "ogg" => {
                    cmd = cmd.arg("-c:a").arg("libvorbis");
                }
                "opus" => {
                    cmd = cmd.arg("-c:a").arg("libopus")
                }
                "wav" => {
                    cmd = cmd.arg("-c:a").arg("pcm_s16le");
                }
                "weba" => {
                    cmd = cmd
                        .arg("-c:a").arg("libopus")
                        .arg("-f").arg("webm");
                }
                "flac" => {
                    cmd = cmd.arg("-c:a").arg("flac");
                }
                "aac" | "m4a" => {
                    cmd = cmd.arg("-c:a").arg("aac");
                }
                "aiff" => {
                    cmd = cmd.arg("-c:a").arg("pcm_s16be");
                }
                _ => {}
            }

            match cmd.output_as_file(output_path.clone().into()).done().start() {
                Ok(proc) => proc,
                Err(error) => {
                    eprintln!("Failed to start FFmpeg: {}", error);
                    return Err(
                        HttpResponse::InternalServerError()
                            .content_type("text/plain")
                            .body("Internal Server Error")
                    );
                }
            }
        };

        match timeout(Duration::from_secs(120), web::block(move || ffmpeg.wait())).await {
            Ok(Ok(Ok(status))) => {
                if !status.success() {
                    eprintln!("FFmpeg conversion {} failed with status: {}", conversion_id, status);
                    return Err(
                        HttpResponse::InternalServerError()
                            .content_type("text/plain")
                            .body("Internal Server Error")
                    );
                }
            }
            Ok(Ok(Err(error))) => {
                eprintln!("FFmpeg exited with error: {}", error);
                return Err(
                    HttpResponse::InternalServerError()
                        .content_type("text/plain")
                        .body("Internal Server Error")
                );
            }
            Ok(Err(_)) => {
                return Err(
                    HttpResponse::InternalServerError()
                        .content_type("text/plain")
                        .body("Internal Server Error")
                );
            }
            Err(_) => {
                eprintln!("FFmpeg conversion {} timed out", conversion_id);
                return Err(
                    HttpResponse::InternalServerError()
                        .content_type("text/plain")
                        .body("Internal Server Error")
                );
            }
        }

        let file = match tokio::fs::File::open(&output_path).await {
            Ok(file) => file,
            Err(error) => {
                eprintln!("Failed to open output file: {}", error);
                return Err(
                    HttpResponse::InternalServerError()
                        .content_type("text/plain")
                        .body("Internal Server Error")
                );
            }
        };

        let stream = ReaderStream::new(file);

        println!("Successfully converted {} from {} to {}!", conversion_id, input_extension, format);

        Ok(HttpResponse::Ok()
            .content_type(format_to_mime(&format))
            .insert_header(("Access-Control-Allow-Origin", "*"))
            .streaming(stream))
    }.await;

    // Cleanup -input and -output
    for path in [&input_path, &output_path] {
        if let Err(error) = fs::remove_file(path) {
            eprintln!("Failed to delete file {:#?}: {}", path, error);
        }
    }

    result.unwrap_or_else(|resp| resp)
}
