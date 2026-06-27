//! Bottom dock shell — tab bar, resize handle, and active-tab content only.

use std::sync::Arc;

use gpui::{
    div, px, svg, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::bottom_panel::{BottomPanelResizeDrag, BottomTab};
use crate::components::effect_editor_tab_view::EffectEditorTabView;
use crate::components::editor_panel::ClipEditorPanel;
use crate::components::mixer_panel_view::{docked_mixer_shell, MixerPanelView};
use crate::layout::StudioLayout;
use crate::theme::Colors;

const TABBAR_H: f32 = 28.0;

pub struct BottomPanelShell {
    owner: Entity<StudioLayout>,
    mixer_panel: Entity<MixerPanelView>,
    clip_editor: Entity<ClipEditorPanel>,
    effect_editor: Entity<EffectEditorTabView>,
    last_shell_key: u64,
}

impl BottomPanelShell {
    pub fn new(
        owner: Entity<StudioLayout>,
        mixer_panel: Entity<MixerPanelView>,
        clip_editor: Entity<ClipEditorPanel>,
        effect_editor: Entity<EffectEditorTabView>,
    ) -> Self {
        Self {
            owner,
            mixer_panel,
            clip_editor,
            effect_editor,
            last_shell_key: u64::MAX,
        }
    }

    fn shell_key(owner: &StudioLayout) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let q = |v: f32| (v * 4.0).round() as i64;
        owner.active_bottom_tab().hash(&mut hasher);
        q(owner.bottom_panel_state().height_px).hash(&mut hasher);
        owner.bottom_panel_docked().hash(&mut hasher);
        owner.mixer_tree_sidebar_enabled().hash(&mut hasher);
        hasher.finish()
    }
}

impl Render for BottomPanelShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _scope = crate::perf::PerfScope::enter("BottomPanelShell");
        crate::perf::count("bottom_panel_root_layout_count", 1);
        crate::perf::count("bottom_panel_root_paint_count", 1);

        let owner = self.owner.read(cx);
        if !owner.bottom_panel_docked() {
            return div().into_any_element();
        }

        let shell_key = Self::shell_key(owner);
        if shell_key != self.last_shell_key {
            self.last_shell_key = shell_key;
        }

        let active_tab = owner.active_bottom_tab();
        crate::perf::count("active_bottom_tab", tab_counter_id(active_tab));

        let panel_state = owner.bottom_panel_state();
        let owner_entity = self.owner.clone();
        let shell_entity = cx.entity();

        let on_tab_click: Arc<dyn Fn(&BottomTab, &mut Window, &mut App) + 'static> =
            Arc::new(move |tab: &BottomTab, _w, cx| {
                let _ = owner_entity.update(cx, |layout, cx| {
                    layout.set_active_bottom_tab(*tab, cx);
                });
            });

        let owner_resize = self.owner.clone();
        let on_resize_start: Arc<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static> =
            Arc::new(move |event, window, cx| {
                let _ = owner_resize.update(cx, |layout, cx| {
                    layout.apply_bottom_panel_resize_start(event, window, cx);
                });
            });
        let owner_move = self.owner.clone();
        let shell_move = shell_entity.clone();
        let on_resize_move: Arc<
            dyn Fn(&gpui::DragMoveEvent<BottomPanelResizeDrag>, &mut Window, &mut App) + 'static,
        > = Arc::new(move |event, _window, cx| {
            let _ = owner_move.update(cx, |layout, cx| {
                if layout.apply_bottom_panel_resize_move(event, cx) {
                    let _ = shell_move.update(cx, |_, cx| cx.notify());
                }
            });
        });
        let owner_end = self.owner.clone();
        let on_resize_end: Arc<dyn Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static> =
            Arc::new(move |_event, _window, cx| {
                let _ = owner_end.update(cx, |layout, cx| layout.apply_bottom_panel_resize_end(cx));
            });

        div()
            .flex()
            .flex_col()
            .h(px(panel_state.height_px))
            .w_full()
            .border_t(px(1.0))
            .border_color(Colors::panel_border())
            .bg(Colors::bottom_panel_bg())
            .relative()
            .on_drag_move::<BottomPanelResizeDrag>({
                let handler = on_resize_move.clone();
                move |event, window, cx| handler(event, window, cx)
            })
            .on_mouse_up(gpui::MouseButton::Left, {
                let handler = on_resize_end.clone();
                move |event, window, cx| handler(event, window, cx)
            })
            .child(render_resize_handle(on_resize_start))
            .child(render_tab_bar(active_tab, on_tab_click))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(render_active_tab(
                        active_tab,
                        &self.mixer_panel,
                        &self.clip_editor,
                        &self.effect_editor,
                    )),
            )
            .into_any_element()
    }
}

fn tab_counter_id(tab: BottomTab) -> u64 {
    match tab {
        BottomTab::Mixer => 0,
        BottomTab::Editor => 1,
        BottomTab::EffectEditor => 2,
    }
}

fn render_resize_handle(
    on_resize_start: Arc<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    div()
        .absolute()
        .top(px(-2.0))
        .left_0()
        .right_0()
        .h(px(5.0))
        .id("bottom-panel-resize-handle")
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .hover(|s| s.bg(Colors::accent_soft()))
        .on_mouse_down(gpui::MouseButton::Left, {
            let handler = on_resize_start.clone();
            move |event, window, cx| handler(event, window, cx)
        })
        .on_drag(BottomPanelResizeDrag, |_drag, _offset, _window, cx| {
            cx.new(|_| BottomPanelResizeDrag)
        })
}

fn render_tab_bar(
    active_tab: BottomTab,
    on_tab_click: Arc<dyn Fn(&BottomTab, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let _scope = crate::perf::PerfScope::enter("BottomPanelTabBar");
    crate::perf::count("bottom_panel_tabbar_layout_count", 1);
    crate::perf::count("bottom_panel_tabbar_paint_count", 1);

    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(TABBAR_H))
        .px(px(8.0))
        .border_b(px(1.0))
        .border_color(Colors::panel_border())
        .bg(Colors::bottom_panel_header_bg())
        .child(tab_button(
            "Mixer",
            assets::ICON_SLIDERS_HORIZONTAL_PATH,
            BottomTab::Mixer,
            active_tab,
            on_tab_click.clone(),
        ))
        .child(tab_button(
            "Editor",
            assets::ICON_PENCIL_PATH,
            BottomTab::Editor,
            active_tab,
            on_tab_click.clone(),
        ))
        .child(tab_button(
            "Effect Editor",
            assets::ICON_SPARKLES_PATH,
            BottomTab::EffectEditor,
            active_tab,
            on_tab_click,
        ))
}

fn tab_button(
    label: &'static str,
    icon_path: &'static str,
    tab: BottomTab,
    active_tab: BottomTab,
    on_click: Arc<dyn Fn(&BottomTab, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let active = tab == active_tab;
    let on_click_clone = on_click.clone();
    let text_color = if active {
        Colors::text_primary()
    } else {
        Colors::text_muted()
    };

    let mut btn = div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(24.0))
        .px(px(10.0))
        .rounded_md()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(text_color)
        .id(label)
        .on_click(move |_, window, cx| {
            on_click_clone(&tab, window, cx);
        })
        .child(
            svg()
                .path(icon_path)
                .w(px(14.0))
                .h(px(14.0))
                .text_color(text_color),
        )
        .child(label);

    if active {
        btn = btn
            .bg(Colors::surface_hover())
            .child(
                div()
                    .absolute()
                    .bottom(px(0.0))
                    .left(px(6.0))
                    .right(px(6.0))
                    .h(px(2.0))
                    .bg(Colors::accent_primary()),
            );
    } else {
        btn = btn.hover(|style| {
            style
                .bg(Colors::surface_hover())
                .text_color(Colors::text_secondary())
        });
    }
    btn
}

fn render_active_tab(
    active_tab: BottomTab,
    mixer_panel: &Entity<MixerPanelView>,
    clip_editor: &Entity<ClipEditorPanel>,
    effect_editor: &Entity<EffectEditorTabView>,
) -> gpui::AnyElement {
    let _scope = crate::perf::PerfScope::enter("BottomPanelContent");
    crate::perf::count("bottom_panel_content_layout_count", 1);
    crate::perf::count("bottom_panel_content_paint_count", 1);

    match active_tab {
        BottomTab::Mixer => docked_mixer_shell(mixer_panel.clone()).into_any_element(),
        BottomTab::Editor => clip_editor.clone().into_any_element(),
        BottomTab::EffectEditor => effect_editor.clone().into_any_element(),
    }
}
