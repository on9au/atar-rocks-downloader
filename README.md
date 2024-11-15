# ATAR.Rocks downloader

A web-crawler/script to download the files from [atar.rocks](https://atar.rocks).

## Features

- Ability to save and load caches, preventing the need to rescan the website.
- Informative progress bar and logging.
- Fast and efficient, thanks to the Rust programming language. (my code still sucks though)

## Usage

Please look at the wiki for detailed instructions on how to use the script.

## I'm a beginner, how can I run this script?

1. Go to the **[Actions](https://github.com/nulluser0/atar-rocks-downloader/actions)** tab of this repository.
2. Click on the workflow run that you are interested in.
3. Scroll down to the **Artifacts** section at the bottom of the workflow summary.
4. Click on the artifact you want to download.
5. For:
   - `Windows`: Just run the file.
   - `Linux`: Run the file using `./<filename>`.
   - `MacOS`: Open the Terminal app, navigate to where you installed the program, and run the file using `./<filename>`.
6. Follow the instructions on the screen.

## I know what I'm doing, how can I build this script?

Prerequisites:

- [Rust](https://www.rust-lang.org/tools/install)
- [Git](https://git-scm.com/downloads)

1. Clone the repository using `git clone`.
2. Navigate to the cloned repository using `cd atar-rocks-downloader`.
3. Run the script using `cargo run --release`.
4. The binary will be available in the `target/release` directory.

## License

This project is licensed under the MIT License - see the [LICENSE](/LICENSE) file for details.
