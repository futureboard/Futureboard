#![allow(dead_code)]
#![allow(
    clippy::arc_with_non_send_sync,
    clippy::clone_on_copy,
    clippy::collapsible_match,
    clippy::derivable_impls,
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items,
    clippy::double_ended_iterator_last,
    clippy::enum_variant_names,
    clippy::excessive_precision,
    clippy::field_reassign_with_default,
    clippy::let_unit_value,
    clippy::manual_clamp,
    clippy::manual_is_multiple_of,
    clippy::manual_map,
    clippy::manual_pattern_char_comparison,
    clippy::manual_range_contains,
    clippy::module_inception,
    clippy::needless_borrow,
    clippy::needless_borrows_for_generic_args,
    clippy::needless_lifetimes,
    clippy::needless_return,
    clippy::new_without_default,
    clippy::ptr_arg,
    clippy::single_match,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_min_or_max,
    clippy::unnecessary_sort_by,
    clippy::useless_conversion,
    clippy::useless_format
)]

pub mod app_state;
pub mod assets;
pub mod audio_routing;
pub mod boot;
pub mod color;
pub mod components;
pub mod embedded_assets;
pub mod feeds;
pub mod forensic_trace;
pub mod i18n;
pub mod keymap;
pub mod layout;
pub mod menu;
pub mod midi_devices;
pub mod native_macos_menu;
pub mod overlay;
pub mod paths;
pub mod perf;
pub mod platform_chrome;
pub mod project;
pub mod settings;
pub mod shutdown;
pub mod window_position;
pub use shutdown::ShutdownState;
/// Re-export of the separated plugin-host bridge client so the native app can
/// log bridge env / drive the bridge without a direct `sphere-plugin-host` dep.
pub use sphere_plugin_host::plugin_host_client;
pub use sphere_plugin_host::plugin_host_lifecycle;
pub mod splash;
pub mod theme;
pub mod welcome;

pub fn ui_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("FUTUREBOARD_UI_DEBUG")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}
