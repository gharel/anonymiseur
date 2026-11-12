#![allow(clippy::print_stdout)]

//! Compile la cale C d'OCR (`csrc/ocr_shim.c`) contre les en-têtes de MuPDF
//! fournis par `mupdf-sys`.
//!
//! `mupdf-sys` ne publie pas son dossier d'en-têtes (`cargo:include`) : on le
//! cherche, dans l'ordre, dans `ANON_MUPDF_INCLUDE`, dans le dossier de build de
//! `mupdf-sys` voisin du nôtre, puis dans les sources du registre Cargo.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MUPDF_SYS_VERSION: &str = "0.8.0";

fn has_fitz(dir: &Path) -> bool {
    dir.join("mupdf").join("fitz.h").is_file()
}

fn newest_matching(parent: &Path, prefix: &str, sub: &[&str]) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(parent).ok()?.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(prefix) {
            continue;
        }
        let mut cand = entry.path();
        for s in sub {
            cand.push(s);
        }
        if !has_fitz(&cand) {
            continue;
        }
        let mtime = fs::metadata(cand.join("mupdf").join("fitz.h"))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, cand));
        }
    }
    best.map(|(_, p)| p)
}

fn find_include() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ANON_MUPDF_INCLUDE") {
        let p = PathBuf::from(p);
        if has_fitz(&p) {
            return Some(p);
        }
    }
    // OUT_DIR = <target>/<profil>/build/anon-pdf-<hash>/out
    if let Some(build_dir) = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .and_then(|o| o.parent().and_then(Path::parent).map(Path::to_path_buf))
        && let Some(p) = newest_matching(&build_dir, "mupdf-sys-", &["out", "build", "include"])
    {
        return Some(p);
    }
    let cargo_home = env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
        env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .map(|h| PathBuf::from(h).join(".cargo"))
    })?;
    let src = cargo_home.join("registry").join("src");
    for entry in fs::read_dir(&src).ok()?.flatten() {
        let cand = entry
            .path()
            .join(format!("mupdf-sys-{MUPDF_SYS_VERSION}"))
            .join("mupdf")
            .join("include");
        if has_fitz(&cand) {
            return Some(cand);
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=csrc/ocr_shim.c");
    println!("cargo:rerun-if-env-changed=ANON_MUPDF_INCLUDE");
    let Some(include) = find_include() else {
        println!(
            "cargo:warning=en-têtes MuPDF introuvables : définissez ANON_MUPDF_INCLUDE (voir crates/anon-pdf/BUILD.md)"
        );
        std::process::exit(1);
    };
    let mut build = cc::Build::new();
    build
        .file("csrc/ocr_shim.c")
        .include(&include)
        .warnings(false);
    if let Err(e) = build.try_compile("anon_ocr_shim") {
        println!("cargo:warning=compilation de la cale OCR impossible : {e}");
        std::process::exit(1);
    }
}
