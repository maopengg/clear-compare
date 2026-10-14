fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "compare",
            "cancel",
            "page",
            "text_diff",
            "preview_sync",
            "apply_sync",
            "pick_directory",
            "load_session",
            "save_session",
        ]),
    ))
    .expect("Tauri build configuration");
}
