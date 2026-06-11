use std::path::Path;

fn main() {
    // tauri.conf.json lists resources/zotero-notebook.xpi as a bundled
    // resource, and tauri-build fails when a listed resource is missing.
    // The real .xpi is produced by `npm run build:plugin`, which the tauri
    // CLI runs via beforeDev/beforeBuildCommand — but a bare
    // `cargo check`/`cargo build` of this crate (IDE, clippy, future CI
    // lints) has no such hook. Drop an empty placeholder so plain cargo
    // commands work; every bundling path overwrites it first.
    let placeholder = Path::new("resources/zotero-notebook.xpi");
    if !placeholder.exists() {
        std::fs::create_dir_all("resources").expect("create resources dir");
        std::fs::write(placeholder, []).expect("write placeholder xpi");
        println!("cargo:warning=resources/zotero-notebook.xpi was missing; wrote an empty placeholder (run `npm run build:plugin` for the real one)");
    }

    tauri_build::build()
}
