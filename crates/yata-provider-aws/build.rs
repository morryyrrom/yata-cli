//! Build script: discovers collectors under `src/collectors/` and generates the module list.

#[path = "build/discover.rs"]
mod discover;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/discover.rs");
    println!("cargo:rerun-if-changed=src/collectors");

    if let Err(message) = run() {
        eprintln!("error: collector discovery failed: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or("CARGO_MANIFEST_DIR is not set")?;
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or("OUT_DIR is not set")?;
    let entries = discover::discover(&manifest_dir.join("src").join("collectors"))?;
    let code = discover::generate(&entries);
    fs::write(out_dir.join("collectors_generated.rs"), code)
        .map_err(|e| format!("failed to write the generated file: {e}"))
}
