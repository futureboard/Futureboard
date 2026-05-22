use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;
use crate::assets;

pub struct EmbeddedAssets;

impl EmbeddedAssets {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmbeddedAssets {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetSource for EmbeddedAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes = match path {
            assets::ICON_PLAY_PATH => Some(assets::ICON_PLAY),
            assets::ICON_PAUSE_PATH => Some(assets::ICON_PAUSE),
            assets::ICON_SQUARE_PATH => Some(assets::ICON_SQUARE),
            assets::ICON_CIRCLE_PATH => Some(assets::ICON_CIRCLE),
            assets::ICON_SKIP_BACK_PATH => Some(assets::ICON_SKIP_BACK),
            assets::ICON_REPEAT_PATH => Some(assets::ICON_REPEAT),
            assets::ICON_TIMER_PATH => Some(assets::ICON_TIMER),
            assets::ICON_SAVE_PATH => Some(assets::ICON_SAVE),
            assets::ICON_FOLDER_PATH => Some(assets::ICON_FOLDER),
            assets::ICON_SHARE_PATH => Some(assets::ICON_SHARE),
            assets::ICON_MAXIMIZE_PATH => Some(assets::ICON_MAXIMIZE),
            assets::ICON_MINIMIZE_PATH => Some(assets::ICON_MINIMIZE),
            assets::ICON_RESTORE_PATH => Some(assets::ICON_RESTORE),
            assets::ICON_X_PATH => Some(assets::ICON_X),
            assets::ICON_PANEL_BOTTOM_PATH => Some(assets::ICON_PANEL_BOTTOM),
            assets::ICON_MINUS_PATH => Some(assets::ICON_MINUS),
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let all_paths = [
            assets::ICON_PLAY_PATH,
            assets::ICON_PAUSE_PATH,
            assets::ICON_SQUARE_PATH,
            assets::ICON_CIRCLE_PATH,
            assets::ICON_SKIP_BACK_PATH,
            assets::ICON_REPEAT_PATH,
            assets::ICON_TIMER_PATH,
            assets::ICON_SAVE_PATH,
            assets::ICON_FOLDER_PATH,
            assets::ICON_SHARE_PATH,
            assets::ICON_MAXIMIZE_PATH,
            assets::ICON_MINIMIZE_PATH,
            assets::ICON_RESTORE_PATH,
            assets::ICON_X_PATH,
            assets::ICON_PANEL_BOTTOM_PATH,
            assets::ICON_MINUS_PATH,
        ];
        let mut list = Vec::new();
        for p in all_paths {
            if p.starts_with(path) {
                list.push(SharedString::from(p));
            }
        }
        Ok(list)
    }
}
