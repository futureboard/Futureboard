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

pub mod assets;
pub mod boot;
pub mod components;
pub mod embedded_assets;
pub mod layout;
pub mod menu;
pub mod native_macos_menu;
pub mod overlay;
pub mod paths;
pub mod perf;
pub mod platform_chrome;
pub mod project;
pub mod settings;
pub mod splash;
pub mod theme;
