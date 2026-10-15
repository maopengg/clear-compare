//! Presentation preferences in this WebView's local storage; no file paths or contents.
use serde::{Deserialize, Serialize};
const KEY: &str = "clear-compare.presentation.v1";
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}
impl Theme {
    pub fn value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
    pub fn from_value(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    theme: Theme,
}
fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("Window unavailable")?
        .local_storage()
        .map_err(|_| "Presentation storage is unavailable".to_string())?
        .ok_or("Presentation storage is disabled".into())
}
pub fn load() -> Result<Theme, String> {
    let Some(raw) = storage()?
        .get_item(KEY)
        .map_err(|_| "Cannot read presentation settings")?
    else {
        return Ok(Theme::System);
    };
    if raw.len() > 4096 {
        return Err("Presentation settings exceed the supported size".into());
    }
    let stored: Stored = serde_json::from_str(&raw)
        .map_err(|_| "Invalid presentation settings; choose a theme to replace them")?;
    if stored.version != 1 {
        return Err(
            "Unsupported presentation settings version; choose a theme to replace it".into(),
        );
    }
    Ok(stored.theme)
}
pub fn save(theme: Theme) -> Result<(), String> {
    let raw = serde_json::to_string(&Stored { version: 1, theme }).map_err(|e| e.to_string())?;
    storage()?
        .set_item(KEY, &raw)
        .map_err(|_| "Cannot persist theme; this choice applies until the app closes".into())
}
