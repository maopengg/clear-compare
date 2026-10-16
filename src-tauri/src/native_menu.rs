//! Preserve platform-native edit/window actions and their keyboard shortcuts.
use tauri::{
    AppHandle,
    menu::{Menu, MenuItemKind},
};
fn chinese(text: &str) -> String {
    let plain = text.replace('&', "");
    match plain.as_str() {
        "File" => "文件",
        "Edit" => "编辑",
        "View" => "显示",
        "Window" => "窗口",
        "Help" => "帮助",
        "Undo" => "撤销",
        "Redo" => "重做",
        "Cut" => "剪切",
        "Copy" => "复制",
        "Paste" => "粘贴",
        "Paste and Match Style" => "粘贴并匹配样式",
        "Select All" => "全选",
        "Delete" => "删除",
        "Minimize" => "最小化",
        "Zoom" | "Maximize" => "缩放",
        "Toggle Full Screen" => "切换全屏",
        "Close" | "Close Window" => "关闭窗口",
        "Quit" | "Exit" => "退出",
        "About" => "关于 Clear Compare",
        "Services" => "服务",
        "Hide" => "隐藏 Clear Compare",
        "Hide Others" => "隐藏其他",
        "Show All" => "全部显示",
        "Bring All to Front" => "全部置于前面",
        "About Clear Compare" => "关于 Clear Compare",
        "Hide Clear Compare" => "隐藏 Clear Compare",
        "Quit Clear Compare" => "退出 Clear Compare",
        _ => return text.to_owned(),
    }
    .into()
}
fn translate(items: Vec<MenuItemKind<tauri::Wry>>) -> tauri::Result<()> {
    for item in items {
        match item {
            MenuItemKind::Submenu(s) => {
                s.set_text(chinese(&s.text()?))?;
                translate(s.items()?)?;
            }
            MenuItemKind::Predefined(p) => {
                let text = p.text()?;
                if !text.is_empty() {
                    p.set_text(chinese(&text))?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn set(app: &AppHandle, english: bool) -> tauri::Result<()> {
    let menu = Menu::default(app)?;
    if !english {
        translate(menu.items()?)?;
    }
    app.set_menu(menu)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn localized_native_actions_keep_platform_accelerator_labels_out_of_the_title() {
        for (en, zh) in [
            ("&Copy", "复制"),
            ("Cu&t", "剪切"),
            ("Select &All", "全选"),
            ("Edit", "编辑"),
            ("File", "文件"),
            ("View", "显示"),
            ("Window", "窗口"),
            ("Help", "帮助"),
            ("Undo", "撤销"),
            ("Redo", "重做"),
            ("&Paste", "粘贴"),
            ("C&lose Window", "关闭窗口"),
            ("Toggle Full Screen", "切换全屏"),
        ] {
            assert_eq!(super::chinese(en), zh);
        }
        assert_eq!(super::chinese("Clear Compare"), "Clear Compare");
    }
}
