# audio_converter
An audio conversion API written in [Rust](https://www.rust-lang.org/)! Supports `mp3`, `ogg`, `opus`, `wav`, `weba`, `flac`, `aac`, `m4a` & `aiff`

## Running
1. Download a binary from the **[releases tab](https://github.com/M336G/audio_converter/releases/)** or clone the repository (if you have **[Rust](https://www.rust-lang.org/)** installed).
2. Take a look at **[.env.example](https://github.com/M336G/audio_converter/blob/main/.env.example)** and create a `.env` file if you need to configure your server.
3. Execute the binary you downloaded or use `cargo run --release` if you cloned the repository.

*You may additionally need to forward the port you chose and/or allow incoming requests to that port on your firewall.*

## Usage
Once you've got your instance running, you may use the `POST /` endpoint by supplying a `file` and a `format` to it via multipart form data.

That's it, it's this simple!

## Customizing the frontend
You may also take a look at the **[public folder](https://github.com/M336G/audio_converter/tree/main/public)** to customize your audio_converter further. All the files you add to it while be available publicly on `/`.

## Contributing
Pull requests are more than welcome to the project! Feel free to open one if you feel like something needs modification or if there is any problem with the codebase.

## Credits
This project is licensed under the [Mozilla Public License Version 2.0](https://github.com/M336G/audio_converter/blob/main/LICENSE).