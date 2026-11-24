//! The translator's fingerprint for the shader cache (`shader_cache.rs`):
//! a hash of what decides a translation, the shader crate's sources, the
//! cache's own encoding and the locked dependencies. A rebuild that changes
//! none of them keeps the cache warm; one that changes any starts it afresh.

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let shader = here.join("../victory-shader");
    let mut inputs = Vec::new();
    files(&shader.join("src"), &mut inputs);
    inputs.sort();
    inputs.push(shader.join("Cargo.toml"));
    inputs.push(here.join("src/shader_cache.rs"));
    inputs.push(here.join("../../Cargo.lock"));
    let mut hasher = blake3::Hasher::new();
    for path in &inputs {
        let name = path.strip_prefix(&here).unwrap_or(path);
        hasher.update(name.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update(&[0]);
        if let Ok(bytes) = std::fs::read(path) {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-changed={}", shader.join("src").display());
    println!("cargo:rustc-env=VICTORY_TRANSLATOR_HASH={}", hasher.finalize().to_hex());
}
