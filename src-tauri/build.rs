fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "compare",
            "cancel",
            "page",
            "text_diff",
            "merge_buffers",
            "binary_page",
            "table_compare",
            "table_page",
            "preview_sync",
            "apply_sync",
            "pick_directory",
            "load_session",
            "save_session",
            "open_document",
            "close_document",
            "save_document",
            "preview_editor_restore",
            "apply_editor_restore",
        ]),
    ))
    .expect("Tauri build configuration");
}
