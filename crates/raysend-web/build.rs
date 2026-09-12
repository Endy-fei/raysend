use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let images_dir = Path::new("public/assets/images");
    fs::create_dir_all(images_dir).expect("Failed to create images assets directory");

    let github_logo_url =
        "https://github.githubassets.com/images/modules/logos_page/GitHub-Mark.png";
    download_file(github_logo_url, images_dir.join("GitHub-Mark.png"));

    build_decode_wasm();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../raysend-decode");
    println!("cargo:rerun-if-changed=../raysend-core/src/scan.rs");
    println!("cargo:rerun-if-changed=../raysend-core/src/scan_tracked.rs");
    println!("cargo:rerun-if-changed=../raysend-core/Cargo.toml");
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

fn build_decode_wasm() {
    if std::env::var_os("SKIP_DECODE_WASM").is_some() {
        return;
    }

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("raysend-web should live in crates/")
        .to_path_buf();
    let decode_target = workspace.join("target/decode-wasm");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    let status = Command::new(&cargo)
        .args([
            "build",
            "-p",
            "raysend-decode",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--manifest-path",
        ])
        .arg(workspace.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &decode_target)
        .env("CARGO_PROFILE_RELEASE_LTO", "true")
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
        .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "s")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("failed to spawn cargo for raysend-decode");
    if !status.success() {
        panic!("raysend-decode wasm build failed: {status}");
    }

    let wasm_path = decode_target
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("raysend_decode.wasm");
    if !wasm_path.exists() {
        panic!("missing {}", wasm_path.display());
    }

    let out_dir = manifest_dir.join("public/qr-decode");
    fs::create_dir_all(&out_dir).expect("create public/qr-decode");

    let mut bindgen = wasm_bindgen_cli_support::Bindgen::new();
    bindgen
        .input_path(&wasm_path)
        .out_name("qr-decode")
        .web(true)
        .expect("wasm-bindgen --target web")
        .typescript(false)
        .remove_name_section(true)
        .remove_producers_section(true)
        .generate(&out_dir)
        .unwrap_or_else(|err| panic!("wasm-bindgen raysend-decode failed: {err}"));
}
