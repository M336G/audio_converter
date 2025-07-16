# audio-converter
An audio conversion API written in Rust! Supports .mp3, .ogg, .opus, .wav & .webm!

## Usage
1. Download a binary from the **[releases tab](https://github.com/M336G/audio-converter/releases/))** or clone the repository (if you have **[Rust](https://www.rust-lang.org/)** installed).
2. Take a look at **[.env.example](https://github.com/M336G/audio-converter/blob/main/.env.example))** and create a `.env` file if you need to configure your server.
3. Execute the binary you downloaded or use `cargo run --release` if you cloned the repository.
*You may additionally need to forward the port you chose and/or allow incoming requests to that port on your firewall.*

## Customizing the frontend
You may also take a look at the **[public folder](https://github.com/M336G/audio-converter/tree/main/public))** to customize your audio-converter further. All the files you add to it while be available publicly on `/`.

## Contributing
Pull requests are more than welcome to the project! Feel free to open one if you feel like something needs modification or if there are problems with the codebase.

## License
This project is licensed under the [Mozilla Public License Version 2.0](https://github.com/M336G/audio-converter/blob/main/LICENSE).