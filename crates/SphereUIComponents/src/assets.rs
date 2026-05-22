// Embedded fonts — loaded from packages/shared/fonts via include_bytes!
pub const INTER_REGULAR: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-Regular.ttf");
pub const INTER_MEDIUM: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-Medium.ttf");
pub const INTER_SEMIBOLD: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-SemiBold.ttf");
pub const INTER_BOLD: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-Bold.ttf");

// Transport icons (Lucide SVG bytes)
pub const ICON_PLAY: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/play.svg");
pub const ICON_PAUSE: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/pause.svg");
pub const ICON_SQUARE: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/square.svg");
pub const ICON_CIRCLE: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/circle.svg");
pub const ICON_SKIP_BACK: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/skip-back.svg");
pub const ICON_REPEAT: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/repeat.svg");
pub const ICON_TIMER: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/timer.svg");
pub const ICON_SAVE: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/save.svg");
pub const ICON_FOLDER: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/folder.svg");
pub const ICON_SHARE: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/share.svg");
pub const ICON_MAXIMIZE: &[u8] =
    include_bytes!("../../../packages/shared/icons/generic_maximize.svg");
pub const ICON_MINIMIZE: &[u8] =
    include_bytes!("../../../packages/shared/icons/generic_minimize.svg");
pub const ICON_RESTORE: &[u8] =
    include_bytes!("../../../packages/shared/icons/generic_restore.svg");
pub const ICON_X: &[u8] =
    include_bytes!("../../../packages/shared/icons/generic_close.svg");
pub const ICON_PANEL_BOTTOM: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/panel-bottom.svg");
pub const ICON_MINUS: &[u8] =
    include_bytes!("../../../packages/shared/lucide/icons/minus.svg");

// SVG virtual path constants
pub const ICON_PLAY_PATH: &str = "icons/play.svg";
pub const ICON_PAUSE_PATH: &str = "icons/pause.svg";
pub const ICON_SQUARE_PATH: &str = "icons/square.svg";
pub const ICON_CIRCLE_PATH: &str = "icons/circle.svg";
pub const ICON_SKIP_BACK_PATH: &str = "icons/skip-back.svg";
pub const ICON_REPEAT_PATH: &str = "icons/repeat.svg";
pub const ICON_TIMER_PATH: &str = "icons/timer.svg";
pub const ICON_SAVE_PATH: &str = "icons/save.svg";
pub const ICON_FOLDER_PATH: &str = "icons/folder.svg";
pub const ICON_SHARE_PATH: &str = "icons/share.svg";
pub const ICON_MAXIMIZE_PATH: &str = "icons/generic_maximize.svg";
pub const ICON_MINIMIZE_PATH: &str =  "icons/generic_minimize.svg";
pub const ICON_RESTORE_PATH: &str = "icons/generic_restore.svg";
pub const ICON_X_PATH: &str = "icons/generic_close.svg";
pub const ICON_PANEL_BOTTOM_PATH: &str = "icons/panel-bottom.svg";
pub const ICON_MINUS_PATH: &str = "icons/minus.svg";

/// Registers the embedded Inter fonts with the platform's text system.
pub fn register_fonts(cx: &mut gpui::App) {
    use std::borrow::Cow;
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(INTER_REGULAR),
            Cow::Borrowed(INTER_MEDIUM),
            Cow::Borrowed(INTER_SEMIBOLD),
            Cow::Borrowed(INTER_BOLD),
        ])
        .expect("failed to load fonts");
}

