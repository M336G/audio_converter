use actix_web::{post, web, HttpResponse, Responder};
use essi_ffmpeg::FFmpeg;
use randomizer::Randomizer;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;

#[derive(Deserialize)]
pub struct ConvertParams {
    format: Option<String>,
}

fn format_to_mime(format: &str) -> &'static str {
    match format {
        "mp3" => "audio/mpeg",
        "opus" => "audio/ogg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

#[post("/")]
pub async fn convert(body: web::Bytes, query: web::Query<ConvertParams>) -> impl Responder {
    let format = query.format.clone().unwrap_or_else(|| "mp3".to_string());

    let supported_formats: [&'static str; 5] = ["mp3", "ogg", "opus", "wav", "webm"];

    if !supported_formats.contains(&format.as_str()) {
        return HttpResponse::UnsupportedMediaType()
            .content_type("text/html")
            .body(format!("Unsupported output format: {}", format));
    }

    if body.is_empty() {
        return HttpResponse::BadRequest()
        .content_type("text/html")
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
                    .content_type("text/html")
                    .body("You're trying to convert a file to the same format!");
            }

            if !supported_formats.contains(&ext) {
                return HttpResponse::UnsupportedMediaType()
                    .content_type("text/html")
                    .body(format!("Unsupported input file type: {} ({})", ext, mime));
            }
            ext
        }
        None => return HttpResponse::BadRequest().body("Could not detect input file type"),
    };

    let conversion_id = Randomizer::ALPHANUMERIC(16).string().unwrap();
    let input_path = format!("conversions/{}-input.{}", conversion_id, input_extension);
    let output_path = format!("conversions/{}-output.{}", conversion_id, format);

    println!("New conversion request:");
    println!("- ID: {}", conversion_id);
    println!("- Size: {:.2} MB", body.len() as f32 / 1024.0 / 1024.0);
    println!("- From: {}", input_extension);
    println!("- To: {}", format);

    // Ensure the "conversions" directory exists
    if let Err(err) = fs::create_dir_all("conversions") {
        eprintln!("Failed to create conversions dir: {}", err);
        return HttpResponse::InternalServerError()
            .content_type("text/html")
            .body("Internal Server Error");
    }

    // Create the input file and write to it
    let result = (|| {
        let mut file = File::create(&input_path).map_err(|error| {
            eprintln!("File create error: {}", error);
            HttpResponse::InternalServerError()
                .content_type("text/html")
                .body("Internal Server Error")
        })?;

        file.write_all(&body).map_err(|error| {
            eprintln!("Write error: {}", error);
            HttpResponse::InternalServerError()
                .content_type("text/html")
                .body("Could not write file to convert")
        })?;

        println!("Converting {} from {} to {}...", conversion_id, input_extension, format);

        // Convert the file
        let mut ffmpeg = {
            let mut cmd = FFmpeg::new()
                .stderr(std::process::Stdio::null())
                .input_with_file(input_path.clone().into())
                .done();

            match format.as_str() {
                "ogg" => {
                    cmd = cmd.arg("-c:a").arg("libvorbis");
                }
                "opus" => {
                    cmd = cmd.arg("-c:a").arg("libopus");
                }
                _ => {}
            }

            cmd.output_as_file(output_path.clone().into())
                .done()
                .start()
                .unwrap()
        };

        ffmpeg.wait().unwrap();

        let output_data = fs::read(&output_path).map_err(|error| {
            eprintln!("Read error: {}", error);
            HttpResponse::InternalServerError()
                .content_type("text/html")
                .body("Internal Server Error")
        })?;

        println!("Successfully converted {} from {} to {}!", conversion_id, input_extension, format);

        Ok(HttpResponse::Ok()
            .content_type(format_to_mime(&format))
            .body(output_data))
    })();

    // Cleanup -input and -output
    for path in [&input_path, &output_path] {
        if let Err(error) = fs::remove_file(path) {
            eprintln!("Failed to delete file {}: {}", path, error);
        }
    }

    result.unwrap_or_else(|resp| resp)
}
