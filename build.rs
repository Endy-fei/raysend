use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn main() {
    let images_dir = Path::new("public/assets/images");
    fs::create_dir_all(images_dir).expect("Failed to create images assets directory");

    let github_logo_url =
        "https://github.githubassets.com/images/modules/logos_page/GitHub-Mark.png";
    download_file(github_logo_url, images_dir.join("GitHub-Mark.png"));

    println!("cargo:rerun-if-changed=build.rs");
}

fn download_file(url: &str, dest: impl AsRef<Path>) {
    if dest.as_ref().exists() {
        println!(
            "File already exists: {:?}, skipping download",
            dest.as_ref()
        );
        return;
    }

    println!("Downloading {} to {:?}", url, dest.as_ref());

    let response =
        reqwest::blocking::get(url).unwrap_or_else(|_| panic!("Failed to download {}", url));

    let mut file =
        File::create(&dest).unwrap_or_else(|_| panic!("Failed to create {:?}", dest.as_ref()));

    let content = response.bytes().expect("Failed to read response");

    file.write_all(&content).expect("Failed to write file");

    println!("Successfully downloaded {:?}", dest.as_ref());
}
