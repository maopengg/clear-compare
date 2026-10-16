//! Familiar folder and folded-page silhouettes; no font-dependent icon glyphs.
use crate::tr;
use clear_contracts::Kind;
use leptos::prelude::*;
pub fn kind_label(kind: &Kind, en: bool) -> &'static str {
    match kind {
        Kind::Directory => tr(en, "文件夹", "Folder"),
        Kind::Link => tr(en, "符号链接（不跟随）", "Symbolic link (not followed)"),
        Kind::File => tr(en, "文件", "File"),
        Kind::Other => tr(en, "其他类型", "Other type"),
    }
}
pub fn icon(kind: &Kind, path: &str, expanded: bool) -> impl IntoView + use<> {
    let folder = *kind == Kind::Directory;
    let link = *kind == Kind::Link;
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let detail = if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico"
    ) {
        "M4 12l3-4 2 2 2-3 2 5z M6 5h.01"
    } else if matches!(
        ext.as_str(),
        "rs" | "py"
            | "js"
            | "ts"
            | "vue"
            | "html"
            | "css"
            | "json"
            | "yml"
            | "yaml"
            | "sh"
            | "toml"
    ) {
        "M6 7L4 9l2 2m4-4l2 2-2 2M9 6l-2 6"
    } else if matches!(ext.as_str(), "zip" | "tar" | "gz" | "7z" | "rar") {
        "M7 4h2M7 6h2M7 8h2M7 10h2M7 12h2"
    } else {
        "M4 7h7M4 9h7M4 11h5"
    };
    view! {<svg class=if folder{"entry-icon folder-icon"}else{"entry-icon file-icon"} data-kind=if folder{"folder"}else if link{"link"}else{"file"} data-expanded=expanded.to_string() width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true" focusable="false">
        {if folder {view!{<path class="folder-back" d="M1.2 4.2c0-.7.5-1.2 1.2-1.2h3l1.4 1.5h6.8c.7 0 1.2.5 1.2 1.2v6.8c0 .7-.5 1.2-1.2 1.2H2.4c-.7 0-1.2-.5-1.2-1.2z"/><path class="folder-front" d=if expanded{"M3.2 7h11.2l-1.5 6.5H1.5z"}else{"M1.2 6h13.6v6.5c0 .6-.5 1-1.1 1H2.3c-.6 0-1.1-.4-1.1-1z"}/>} .into_any()}else{view!{<path class="file-paper" d="M3 1.5h6l4 4v9H3z"/><path class="file-fold" d="M9 1.5v4h4"/><path class="file-detail" d=detail/>{link.then(||view!{<path class="file-link" d="M1 13h5m-2-2l2 2-2 2"/>})}}.into_any()}}
    </svg>}
}
