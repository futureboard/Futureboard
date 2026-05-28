//! macOS native menubar — maps [`MenuManifest`] to GPUI `cx.set_menus`.
//!
//! Command dispatch uses shared manifest command IDs so keyboard shortcuts and
//! GPUI dropdowns stay aligned with the system menu.

use std::sync::{Arc, Mutex, OnceLock};

use gpui::App;

static COMMAND_DISPATCHER: OnceLock<Mutex<Option<Arc<dyn Fn(&str, &mut App) + Send + Sync>>>> =
    OnceLock::new();

fn dispatcher_slot() -> &'static Mutex<Option<Arc<dyn Fn(&str, &mut App) + Send + Sync>>> {
    COMMAND_DISPATCHER.get_or_init(|| Mutex::new(None))
}

/// Register the handler that runs menu command IDs (typically `StudioLayout`).
pub fn set_command_dispatcher(dispatcher: Arc<dyn Fn(&str, &mut App) + Send + Sync>) {
    *dispatcher_slot().lock().expect("menu dispatcher lock") = Some(dispatcher);
}

/// Install the application menu from the shared manifest. No-op off macOS.
pub fn install_native_macos_menu(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        if !crate::platform_chrome::PlatformChromePolicy::current().use_native_macos_menubar {
            return;
        }
        install_native_macos_menu_inner(cx);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

#[cfg(target_os = "macos")]
mod macos {
    use gpui::{App, Menu, MenuItem as GpuiMenuItem, SharedString};

    use crate::menu::{MenuItem as AppMenuItem, MenuItemKind, MenuManifest};

    #[derive(Clone, PartialEq, gpui::Action)]
    #[action(no_json)]
    pub(super) struct RunMenuCommand {
        pub command_id: SharedString,
    }

    pub(super) fn install(cx: &mut App) {
        cx.on_action(|action: &RunMenuCommand, cx: &mut App| {
            let command_id = action.command_id.to_string();
            if let Some(dispatcher) = super::dispatcher_slot()
                .lock()
                .ok()
                .and_then(|g| g.clone())
            {
                dispatcher(&command_id, cx);
            } else {
                eprintln!("[macos-menu] no dispatcher for command {command_id}");
            }
        });

        let manifest = MenuManifest::load();
        let menus: Vec<Menu> = manifest
            .menus
            .iter()
            .map(|menu| Menu {
                name: menu.label.clone().into(),
                items: convert_items(&menu.items),
            })
            .collect();

        cx.set_menus(menus);
    }

    fn convert_items(items: &[AppMenuItem]) -> Vec<GpuiMenuItem> {
        items
            .iter()
            .filter(|item| item.visible)
            .filter_map(convert_item)
            .collect()
    }

    fn convert_item(item: &AppMenuItem) -> Option<GpuiMenuItem> {
        match item.kind {
            MenuItemKind::Separator => Some(GpuiMenuItem::separator()),
            MenuItemKind::Submenu => {
                let label = item.label.clone().unwrap_or_else(|| item.id.clone());
                Some(GpuiMenuItem::submenu(Menu {
                    name: label.into(),
                    items: convert_items(&item.children),
                }))
            }
            MenuItemKind::Normal | MenuItemKind::Checkbox | MenuItemKind::Radio => {
                let command = item.command.as_deref().unwrap_or("noop");
                if command == "noop" && !item.enabled {
                    return None;
                }
                let name = item.label.clone().unwrap_or_else(|| item.id.clone());
                Some(GpuiMenuItem::action(
                    name,
                    RunMenuCommand {
                        command_id: command.into(),
                    },
                ))
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn install_native_macos_menu_inner(cx: &mut App) {
    macos::install(cx);
}
