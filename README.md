# ATAR.Rocks downloader

A web-crawler/script to download the files from [atar.rocks](https://atar.rocks).

## Disclaimer

This software is provided "as-is" and is intended for **educational purposes only**. The developer is not responsible for any **illegal use** of this tool, including but not limited to, downloading or accessing copyrighted content without permission, scraping websites in violation of their terms of service, or any other activities that may violate applicable laws.

By using this tool, you agree to **comply with all applicable laws**, including copyright laws, and the terms and conditions of any websites or services you interact with using this software. The developer does not endorse or encourage the illegal use of this software.

**Use this software at your own risk.** The developer disclaims any liability for any damages, legal consequences, or losses incurred as a result of using this tool for unlawful activities.

If you are unsure about the legal implications of using this tool in your country or jurisdiction, consult with a legal professional before proceeding.

## Features

- Ability to save and load caches, preventing the need to rescan the website.
- Informative progress bar and logging.
- Fast and efficient, thanks to the Rust programming language.

## Usage

Please look at the wiki for detailed instructions on how to use the script.

## I'm a beginner, how can I run this script?

**Latest Stable Version via Releases:**

1. Go to the **[Releases](https://github.com/nulluser0/atar-rocks-downloader/releases/latest)** tab of this repository.
2. Match your OS and device with the files in the asset.
3. Download the file by clicking on the files.
4. For:
   - `Windows`: Just run the file (or open your terminal, navigate to the directory of the file, and run the file using `.\<filename>`).
   - `Linux`: Run the file using `./<filename>`.
   - `MacOS`: Open the Terminal app, navigate to where you installed the program, and run the file using `./<filename>`.
5. Follow the instructions on the screen.

**Latest Dev Version via Actions:**

1. Go to the **[Actions](https://github.com/nulluser0/atar-rocks-downloader/actions)** tab of this repository.
2. Click on the workflow run that is at the top and has a green tick (if there are none, wait until they have completed)
3. Scroll down to the **Artifacts** section at the bottom of the workflow summary.
4. Click on the artifact you want to download.
5. For:
   - `Windows`: Just run the file (or open your terminal, navigate to the directory of the file, and run the file using `.\<filename>`).
   - `Linux`: Run the file using `./<filename>`.
   - `MacOS`: Open the Terminal app, navigate to where you installed the program, and run the file using `./<filename>`.
6. Follow the instructions on the screen.

## I know what I'm doing, how can I build this script?

Prerequisites:

- [Rust](https://www.rust-lang.org/tools/install)
- [Git](https://git-scm.com/downloads)

1. Clone the repository using `git clone`.
2. Navigate to the cloned repository using `cd atar-rocks-downloader`.
3. Build the script using `cargo build --release`. (You can also directly run with `cargo run --release`)
4. The binary will be available in the `target/release` directory.

## License

This project is licensed under the MIT License - see the [LICENSE](/LICENSE) file for details.
