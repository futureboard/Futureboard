use std::time::{Duration, Instant};

use gpui::{Context, DragMoveEvent, MouseDownEvent, Window};

use crate::components::{
    status_content_signature, BottomPanelResizeDrag, BottomPanelState, BottomTab, StatusBarContent,
};

use super::StudioLayout;

impl StudioLayout {
    pub(crate) fn bottom_panel_docked(&self) -> bool {
        self.panels.mixer_docked
    }

    pub(crate) fn active_bottom_tab(&self) -> BottomTab {
        self.active_bottom_tab
    }

    pub(crate) fn bottom_panel_state(&self) -> BottomPanelState {
        self.bottom_panel_state
    }

    pub(crate) fn mixer_tree_sidebar_enabled(&self) -> bool {
        self.mixer_view.tree_sidebar_enabled
    }

    pub(crate) fn apply_bottom_panel_resize_start(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bs = &mut self.bottom_panel_state;
        bs.is_resizing = true;
        bs.resize_start_y = f32::from(event.position.y);
        bs.resize_start_height = bs.height_px;
        let window_h: f32 = window.bounds().size.height.into();
        bs.max_height_px = (window_h * 0.70).max(bs.min_height_px + 40.0);
        let _ = cx;
    }

    /// Returns true when height changed enough to refresh the dock shell.
    pub(crate) fn apply_bottom_panel_resize_move(
        &mut self,
        event: &DragMoveEvent<BottomPanelResizeDrag>,
        cx: &mut Context<Self>,
    ) -> bool {
        let bs = &mut self.bottom_panel_state;
        let cur_y: f32 = event.event.position.y.into();
        let delta = bs.resize_start_y - cur_y;
        let new_h = (bs.resize_start_height + delta).clamp(bs.min_height_px, bs.max_height_px);
        if (new_h - bs.height_px).abs() <= 0.5 {
            return false;
        }
        bs.height_px = new_h;
        self.sync_timeline_chrome_metrics(cx);
        let _ = self.timeline.update(cx, |_, cx| cx.notify());
        true
    }

    pub(crate) fn apply_bottom_panel_resize_end(&mut self, cx: &mut Context<Self>) {
        if self.bottom_panel_state.is_resizing {
            self.bottom_panel_state.is_resizing = false;
            self.sync_timeline_chrome_metrics(cx);
            let _ = self.bottom_panel_shell.update(cx, |_, cx| cx.notify());
        }
    }

    pub(crate) fn sync_timeline_chrome_metrics(&self, cx: &mut Context<Self>) {
        const SIDEBAR_WIDTH: f32 = 272.0;
        const INSPECTOR_WIDTH: f32 = 292.0;
        const STATUS_BAR_HEIGHT: f32 = 22.0;
        let show_browser = self.panels.browser;
        let show_inspector = self.panels.inspector;
        let metrics = crate::components::timeline::TimelineChromeMetrics {
            browser_width: if show_browser { SIDEBAR_WIDTH } else { 0.0 },
            inspector_width: if show_inspector { INSPECTOR_WIDTH } else { 0.0 },
            bottom_panel_height: if self.panels.mixer_docked {
                self.bottom_panel_state.height_px
            } else {
                0.0
            },
            status_bar_height: STATUS_BAR_HEIGHT,
        };
        let project_root = self.project_session.folder_path.clone();
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.set_chrome_metrics(metrics);
            timeline.set_project_root(project_root);
        });
    }

    pub(crate) fn notify_bottom_panel_shell(&self, cx: &mut Context<Self>) {
        if self.panels.mixer_docked {
            let _ = self.bottom_panel_shell.update(cx, |_, cx| cx.notify());
        }
    }

    pub(crate) fn set_active_bottom_tab(&mut self, tab: BottomTab, cx: &mut Context<Self>) {
        if self.active_bottom_tab == tab {
            return;
        }
        self.active_bottom_tab = tab;
        self.ensure_mixer_tree_defaults_once(cx);
        self.ensure_mixer_tree_ui_hooks(cx.entity().clone(), cx);
        self.notify_bottom_panel_shell(cx);
    }

    pub(crate) fn notify_status_bar(&self, cx: &mut Context<Self>) {
        let _ = self.status_bar.update(cx, |_, cx| cx.notify());
    }

    /// Push footer text into the isolated status entity; skips repaint when unchanged.
    pub(crate) fn notify_status_bar_if_changed(&mut self, cx: &mut Context<Self>) {
        const STATUS_COALESCE: Duration = Duration::from_millis(250);
        crate::perf::count("bottom_panel_timer_tick_count", 1);

        let show_perf = self
            .settings
            .read(cx)
            .current
            .performance
            .show_status_performance_metrics;
        let content = self.status_bar_content(show_perf);
        let sig = status_content_signature(&content);

        if sig == self.engine_sync.last_status_sig {
            return;
        }

        let now = Instant::now();
        let perf_only = show_perf
            && content.perf.is_some()
            && self.engine_sync.last_status_left_audio_sig == left_audio_signature(&content)
            && now.duration_since(self.engine_sync.last_status_poll_at) < STATUS_COALESCE;
        if perf_only {
            return;
        }

        self.engine_sync.last_status_sig = sig;
        self.engine_sync.last_status_left_audio_sig = left_audio_signature(&content);
        self.engine_sync.last_status_poll_at = now;

        let _ = self.status_bar.update(cx, |bar, cx| {
            if bar.apply_content(content) {
                cx.notify();
            }
        });
    }

    pub(crate) fn status_bar_show_perf_metrics(&self, cx: &gpui::App) -> bool {
        self.settings
            .read(cx)
            .current
            .performance
            .show_status_performance_metrics
    }

    pub(crate) fn status_bar_perf_popover_open(&self) -> bool {
        self.overlay.perf_metrics_popover_open
    }

    pub(crate) fn status_bar_background_tasks(
        &self,
    ) -> &crate::components::BackgroundTaskStore {
        &self.background_tasks
    }

    pub(crate) fn toggle_status_bar_background_tasks(&mut self, cx: &mut Context<Self>) {
        self.background_tasks.toggle_panel();
        self.notify_status_bar(cx);
    }

    pub(crate) fn cancel_status_bar_background_task(
        &mut self,
        task_id: &str,
        cx: &mut Context<Self>,
    ) {
        self.background_tasks.cancel(task_id);
        self.notify_status_bar(cx);
    }

    pub(crate) fn toggle_status_bar_perf_popover(&mut self, cx: &mut Context<Self>) {
        self.overlay.perf_metrics_popover_open = !self.overlay.perf_metrics_popover_open;
        self.notify_status_bar(cx);
    }
}

fn left_audio_signature(content: &StatusBarContent) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.left.hash(&mut hasher);
    content.audio.hash(&mut hasher);
    hasher.finish()
}
