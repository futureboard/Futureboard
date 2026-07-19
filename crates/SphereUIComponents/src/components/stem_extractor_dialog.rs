//! Native Stem Extractor dialog.
//!
//! Compact Futureboard dialog for offline MDX-NET stem separation (CPU/GPU).
//! Owns serializable [`StemExtractParams`] from SphereAudioProcessor, edits them
//! in the UI, and runs a cancellable background job that never leases GPUI
//! entities during decode/separate/encode work.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
};
use sphere_encoder::{
    create_encoder, AudioEncodeOptions, AudioEncodeSpec, AudioFileFormat, AudioSampleFormat,
};

use crate::components::controls::{fb_button, fb_checkbox, FbButtonKind};
use crate::components::form::select::{select, SelectOption};
use crate::components::progress_dialog::{progress_bar, ProgressBarValue};
use crate::components::title_bar::{external_window_titlebar_compact, TITLEBAR_HEIGHT};
use crate::theme::{self, Colors};
use crate::window_position::{apply_owner_display, centered_window_bounds};

pub const STEM_EXTRACTOR_WINDOW_WIDTH: f32 = 520.0;
const STEM_EXTRACTOR_WINDOW_HEIGHT: f32 = 520.0;
const BODY_PAD: f32 = 14.0;
const ROW_GAP: f32 = 9.0;
const LABEL_W: f32 = 96.0;
const CONTROL_H: f32 = 28.0;
const BUTTON_H: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectField {
    Model,
    Device,
    Quality,
}

pub enum StemExtractJobState {
    Editing,
    Running(SphereAudioProcessor::StemExtractProgress),
    Complete(StemExtractJobSummary),
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct StemExtractJobSummary {
    pub model: SphereAudioProcessor::StemModel,
    pub device: SphereAudioProcessor::InferDevice,
    pub backend: SphereAudioProcessor::InferBackendKind,
    pub output_paths: Vec<PathBuf>,
}

#[derive(Default)]
struct SharedJob {
    progress: Option<SphereAudioProcessor::StemExtractProgress>,
    done: Option<Result<StemExtractJobSummary, String>>,
}

struct ActiveJob {
    shared: Arc<Mutex<SharedJob>>,
    cancel: SphereAudioProcessor::StemExtractCancelToken,
}

/// Plain data captured when the dialog opens — never a live entity borrow.
#[derive(Clone, Debug, Default)]
pub struct StemExtractorDialogDefaults {
    pub project_name: String,
    pub suggested_source: Option<PathBuf>,
    pub suggested_output_dir: Option<PathBuf>,
    pub selected_clip_label: Option<String>,
}

pub struct StemExtractorWindow {
    defaults: StemExtractorDialogDefaults,
    params: SphereAudioProcessor::StemExtractParams,
    source_path: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    state: StemExtractJobState,
    open_select: Option<SelectField>,
    job: Option<ActiveJob>,
    focus_handle: FocusHandle,
    gpu_available: bool,
}

impl StemExtractorWindow {
    pub fn new(defaults: StemExtractorDialogDefaults, cx: &mut Context<Self>) -> Self {
        let source_path = defaults.suggested_source.clone();
        let output_dir = defaults.suggested_output_dir.clone().or_else(|| {
            source_path
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        });
        Self {
            defaults,
            params: SphereAudioProcessor::default_stem_extract_params(),
            source_path,
            output_dir,
            state: StemExtractJobState::Editing,
            open_select: None,
            job: None,
            focus_handle: cx.focus_handle(),
            gpu_available: SphereAudioProcessor::gpu_available(),
        }
    }

    fn close(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if let Some(job) = &self.job {
            job.cancel.cancel();
        }
        window.remove_window();
    }

    fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => {
                if self.open_select.is_some() {
                    self.open_select = None;
                    cx.notify();
                } else if matches!(self.state, StemExtractJobState::Running(_)) {
                    self.request_cancel(cx);
                } else {
                    self.close(window, cx);
                }
                true
            }
            "enter" if matches!(self.state, StemExtractJobState::Editing) => {
                self.start_extract(cx);
                true
            }
            _ => false,
        }
    }

    fn request_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(job) = &self.job {
            job.cancel.cancel();
        }
        cx.notify();
    }

    fn can_extract(&self) -> bool {
        self.source_path.is_some()
            && self.output_dir.is_some()
            && self.params.validate().is_ok()
            && !self.params.stems.is_empty()
    }

    fn browse_source(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "native-dialogs")]
        {
            let entity = cx.entity().clone();
            let start = self
                .source_path
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .or_else(|| self.output_dir.clone())
                .unwrap_or_else(std::env::temp_dir);
            cx.spawn(async move |_this, cx| {
                let result = rfd::AsyncFileDialog::new()
                    .set_title("Choose Source Audio")
                    .set_directory(&start)
                    .add_filter(
                        "Audio",
                        &["wav", "flac", "mp3", "aiff", "aif", "ogg", "m4a", "rauf"],
                    )
                    .pick_file()
                    .await;
                if let Some(handle) = result {
                    let path = handle.path().to_path_buf();
                    let _ = entity.update(cx, |this, cx| {
                        this.source_path = Some(path.clone());
                        if this.output_dir.is_none() {
                            this.output_dir = path.parent().map(|d| d.to_path_buf());
                        }
                        if matches!(this.state, StemExtractJobState::Failed(_)) {
                            this.state = StemExtractJobState::Editing;
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        #[cfg(not(feature = "native-dialogs"))]
        {
            self.state = StemExtractJobState::Failed(
                "Native file dialogs are unavailable in this build.".into(),
            );
            cx.notify();
        }
    }

    fn browse_output(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "native-dialogs")]
        {
            let entity = cx.entity().clone();
            let start = self
                .output_dir
                .clone()
                .unwrap_or_else(std::env::temp_dir);
            cx.spawn(async move |_this, cx| {
                let result = rfd::AsyncFileDialog::new()
                    .set_title("Choose Stem Output Folder")
                    .set_directory(&start)
                    .pick_folder()
                    .await;
                if let Some(handle) = result {
                    let _ = entity.update(cx, |this, cx| {
                        this.output_dir = Some(handle.path().to_path_buf());
                        if matches!(this.state, StemExtractJobState::Failed(_)) {
                            this.state = StemExtractJobState::Editing;
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        #[cfg(not(feature = "native-dialogs"))]
        {
            self.state = StemExtractJobState::Failed(
                "Native file dialogs are unavailable in this build.".into(),
            );
            cx.notify();
        }
    }

    fn start_extract(&mut self, cx: &mut Context<Self>) {
        self.open_select = None;
        if let Err(err) = self.params.validate() {
            self.state = StemExtractJobState::Failed(err.user_message());
            cx.notify();
            return;
        }
        let Some(source_path) = self.source_path.clone() else {
            self.state = StemExtractJobState::Failed("Choose a source audio file.".into());
            cx.notify();
            return;
        };
        let Some(output_dir) = self.output_dir.clone() else {
            self.state = StemExtractJobState::Failed("Choose an output folder.".into());
            cx.notify();
            return;
        };

        let shared = Arc::new(Mutex::new(SharedJob::default()));
        let cancel = SphereAudioProcessor::StemExtractCancelToken::new();
        self.job = Some(ActiveJob {
            shared: shared.clone(),
            cancel: cancel.clone(),
        });
        self.state = StemExtractJobState::Running(SphereAudioProcessor::StemExtractProgress::new(
            SphereAudioProcessor::StemExtractStage::Preparing,
            0.0,
            "Preparing…",
        ));

        let params = self.params.clone();
        let source_stem = source_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "stem".to_string());
        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("fb-stem-extract".to_string())
            .spawn(move || {
                let progress_shared = worker_shared.clone();
                let mut on_progress = move |progress: SphereAudioProcessor::StemExtractProgress| {
                    if let Ok(mut guard) = progress_shared.lock() {
                        guard.progress = Some(progress);
                    }
                };

                let result = (|| -> Result<StemExtractJobSummary, String> {
                    on_progress(SphereAudioProcessor::StemExtractProgress::new(
                        SphereAudioProcessor::StemExtractStage::Preparing,
                        2.0,
                        "Decoding source audio…",
                    ));
                    if cancel.is_cancelled() {
                        return Err("cancelled".into());
                    }
                    let buffer = DirectAudio::load_audio_file(&source_path.to_string_lossy())
                        .map_err(|e| e.to_string())?;
                    let channels = buffer.channels.max(1);
                    let input = SphereAudioProcessor::StemExtractInput::new(
                        buffer.sample_rate,
                        channels,
                        buffer.samples,
                    );
                    let extracted = SphereAudioProcessor::extract_stems(
                        &input,
                        &params,
                        &cancel,
                        &mut on_progress,
                    )
                    .map_err(|e| e.user_message())?;

                    if cancel.is_cancelled() {
                        return Err("cancelled".into());
                    }

                    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
                    let total = extracted.stems.len().max(1);
                    let mut output_paths = Vec::with_capacity(extracted.stems.len());
                    for (index, stem) in extracted.stems.iter().enumerate() {
                        if cancel.is_cancelled() {
                            return Err("cancelled".into());
                        }
                        let percent = 90.0 + (index as f32 / total as f32) * 9.0;
                        on_progress(
                            SphereAudioProcessor::StemExtractProgress::new(
                                SphereAudioProcessor::StemExtractStage::Writing,
                                percent,
                                format!("Writing {}.wav", stem.kind.as_str()),
                            )
                            .with_stem(stem.kind),
                        );
                        let path = output_dir.join(format!(
                            "{}_{}.wav",
                            sanitize_file_stem(&source_stem),
                            stem.kind.file_stem_suffix()
                        ));
                        write_stem_wav(&path, stem)?;
                        output_paths.push(path);
                    }

                    Ok(StemExtractJobSummary {
                        model: extracted.model,
                        device: extracted.device,
                        backend: extracted.backend,
                        output_paths,
                    })
                })();

                if let Ok(mut guard) = worker_shared.lock() {
                    guard.done = Some(result);
                }
            })
            .ok();

        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            loop {
                if crate::shutdown::ShutdownState::global().is_shutting_down() {
                    break;
                }
                executor.timer(std::time::Duration::from_millis(50)).await;
                let keep_going = this
                    .update(cx, |this, cx| this.poll_job(cx))
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    fn poll_job(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        let (progress, done) = {
            let Ok(mut guard) = job.shared.lock() else {
                return true;
            };
            (guard.progress.take(), guard.done.take())
        };
        if let Some(done) = done {
            self.state = match done {
                Ok(summary) => StemExtractJobState::Complete(summary),
                Err(message)
                    if job.cancel.is_cancelled()
                        || message.to_ascii_lowercase().contains("cancel") =>
                {
                    StemExtractJobState::Cancelled
                }
                Err(message) => StemExtractJobState::Failed(message),
            };
            self.job = None;
            cx.notify();
            return false;
        }
        if let Some(progress) = progress {
            self.state = StemExtractJobState::Running(progress);
            cx.notify();
        }
        true
    }

    fn apply_select(&mut self, field: SelectField, value: &str) {
        match field {
            SelectField::Model => {
                if let Some(model) = SphereAudioProcessor::StemModel::parse(value) {
                    self.params.set_model(model);
                }
            }
            SelectField::Device => {
                if let Some(device) = SphereAudioProcessor::InferDevice::parse(value) {
                    self.params.device = device;
                }
            }
            SelectField::Quality => {
                if let Some(quality) = SphereAudioProcessor::StemExtractQuality::parse(value) {
                    self.params.quality = quality;
                }
            }
        }
    }
}

impl Render for StemExtractorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep dialog key focus so Escape/Enter route here (not the studio).
        if !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window, cx);
        }
        let target = cx.entity().clone();
        let body = match &self.state {
            StemExtractJobState::Editing | StemExtractJobState::Failed(_) => {
                self.render_editing(target.clone())
            }
            StemExtractJobState::Running(progress) => {
                self.render_progress(progress.clone(), target.clone())
            }
            StemExtractJobState::Complete(summary) => {
                self.render_complete(summary.clone(), target.clone())
            }
            StemExtractJobState::Cancelled => self.render_terminal(
                "Extraction cancelled.",
                Colors::text_secondary(),
                target.clone(),
            ),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .font(theme::ui_font())
            .bg(Colors::surface_base())
            .overflow_hidden()
            .rounded_md()
            .border(px(1.0))
            .border_color(Colors::border_subtle())
            .shadow(vec![gpui::BoxShadow {
                color: Colors::surface_overlay().into(),
                offset: gpui::point(px(0.0), px(6.0)),
                blur_radius: px(20.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .capture_key_down({
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar_compact(
                "Stem Extractor".to_string(),
                "stem-extractor-close",
                {
                    let target = target.clone();
                    move |window, cx| {
                        let _ = target.update(cx, |this, cx| this.close(window, cx));
                    }
                },
            ))
            .child(
                div()
                    .px(px(BODY_PAD))
                    .pt(px(6.0))
                    .text_size(px(11.0))
                    .text_color(Colors::text_muted())
                    .truncate()
                    .child(self.subtitle()),
            )
            .child(body)
    }
}

impl StemExtractorWindow {
    fn subtitle(&self) -> String {
        if let Some(label) = &self.defaults.selected_clip_label {
            format!("Source clip · {label}")
        } else if let Some(path) = &self.source_path {
            format!(
                "MDX-NET stem separation · {}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            )
        } else {
            "MDX-NET stem separation · CPU / GPU".to_string()
        }
    }

    fn render_editing(&self, target: gpui::Entity<Self>) -> gpui::AnyElement {
        let mut col = div()
            .flex()
            .flex_col()
            .flex_1()
            .px(px(BODY_PAD))
            .py(px(BODY_PAD))
            .gap(px(ROW_GAP))
            .child(section_label("SOURCE"))
            .child(self.path_row(
                "Source",
                self.source_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "No file selected".to_string()),
                "stem-source-browse",
                {
                    let target = target.clone();
                    move |_window, cx| {
                        let _ = target.update(cx, |this, cx| this.browse_source(cx));
                    }
                },
            ))
            .child(self.path_row(
                "Output",
                self.output_dir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "No folder selected".to_string()),
                "stem-output-browse",
                {
                    let target = target.clone();
                    move |_window, cx| {
                        let _ = target.update(cx, |this, cx| this.browse_output(cx));
                    }
                },
            ))
            .child(section_label("MODEL"))
            .child(self.model_row(target.clone()))
            .child(self.device_row(target.clone()))
            .child(self.quality_row(target.clone()))
            .child(section_label("STEMS"))
            .child(self.stems_row(target.clone()))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_faint())
                    .child(model_hint(&self.params)),
            )
            .child(div().flex_1());

        if let StemExtractJobState::Failed(message) = &self.state {
            col = col.child(error_banner(message.clone()));
        } else if !self.can_extract() {
            col = col.child(hint_banner(
                "Choose a source file, output folder, and at least one stem.".into(),
            ));
        }

        col = col.child(self.footer(self.can_extract(), target));
        col.into_any_element()
    }

    fn model_row(&self, target: gpui::Entity<Self>) -> impl IntoElement {
        let options = SphereAudioProcessor::STEM_MODELS
            .iter()
            .map(|info| SelectOption::new(info.id, info.label))
            .collect::<Vec<_>>();
        let control = self.dropdown(
            SelectField::Model,
            "stem-model",
            self.params.model.as_str().to_string(),
            options,
            target,
        );
        self.labeled("Model", control)
    }

    fn device_row(&self, target: gpui::Entity<Self>) -> impl IntoElement {
        let options = vec![
            SelectOption::new("cpu", "CPU"),
            SelectOption::new("gpu", "GPU").disabled(!self.gpu_available),
        ];
        let control = self.dropdown(
            SelectField::Device,
            "stem-device",
            self.params.device.as_str().to_string(),
            options,
            target,
        );
        self.labeled("Device", control)
    }

    fn quality_row(&self, target: gpui::Entity<Self>) -> impl IntoElement {
        let options = vec![
            SelectOption::new("draft", "Draft"),
            SelectOption::new("balanced", "Balanced"),
            SelectOption::new("high", "High"),
        ];
        let control = self.dropdown(
            SelectField::Quality,
            "stem-quality",
            self.params.quality.as_str().to_string(),
            options,
            target,
        );
        self.labeled("Quality", control)
    }

    fn stems_row(&self, target: gpui::Entity<Self>) -> impl IntoElement {
        let stems = self.params.model.default_stems();
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(10.0))
            .children(stems.iter().copied().map(|stem| {
                let checked = self.params.stems.contains(stem);
                let target = target.clone();
                fb_checkbox(
                    format!("stem-toggle-{}", stem.as_str()),
                    stem.label(),
                    checked,
                    true,
                    move |_, _window, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.params.set_stem(stem, !this.params.stems.contains(stem));
                            if matches!(this.state, StemExtractJobState::Failed(_)) {
                                this.state = StemExtractJobState::Editing;
                            }
                            cx.notify();
                        });
                    },
                )
            }))
    }

    fn dropdown(
        &self,
        field: SelectField,
        id: &'static str,
        selected: String,
        options: Vec<SelectOption>,
        target: gpui::Entity<Self>,
    ) -> impl IntoElement {
        let open = self.open_select == Some(field);
        let toggle_target = target.clone();
        let change_target = target;
        let selected_value = selected.clone();
        select(
            id,
            Some(selected.as_str()),
            selected_value,
            options,
            open,
            false,
            Arc::new(move |_, _window, cx| {
                let _ = toggle_target.update(cx, |this, cx| {
                    this.open_select = if this.open_select == Some(field) {
                        None
                    } else {
                        Some(field)
                    };
                    cx.notify();
                });
            }),
            Arc::new(move |value, _window, cx| {
                let value = value.clone();
                let _ = change_target.update(cx, |this, cx| {
                    this.apply_select(field, &value);
                    this.open_select = None;
                    if matches!(this.state, StemExtractJobState::Failed(_)) {
                        this.state = StemExtractJobState::Editing;
                    }
                    cx.notify();
                });
            }),
        )
    }

    fn labeled<E: IntoElement>(&self, label: &str, control: E) -> gpui::Stateful<gpui::Div> {
        div()
            .id(SharedString::from(format!("stem-row-{label}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .w(px(LABEL_W))
                    .flex_none()
                    .text_size(px(11.0))
                    .text_color(Colors::text_secondary())
                    .child(label.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(control.into_any_element()),
            )
    }

    fn path_row(
        &self,
        label: &str,
        path_label: String,
        browse_id: &'static str,
        on_browse: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let control = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h(px(CONTROL_H))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .border(px(1.0))
                    .border_color(Colors::border_subtle())
                    .bg(Colors::surface_input())
                    .text_size(px(11.0))
                    .text_color(Colors::text_primary())
                    .truncate()
                    .child(path_label),
            )
            .child(
                div()
                    .id(browse_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(BUTTON_H))
                    .px(px(10.0))
                    .rounded(px(5.0))
                    .border(px(1.0))
                    .border_color(Colors::border_subtle())
                    .text_size(px(11.0))
                    .text_color(Colors::text_secondary())
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(|s| s.bg(Colors::surface_control_hover()))
                    .on_click(move |_, window, cx| on_browse(window, cx))
                    .child("Browse…"),
            );
        self.labeled(label, control)
    }

    fn footer(&self, can_extract: bool, target: gpui::Entity<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_end()
            .gap(px(8.0))
            .pt(px(8.0))
            .border_t(px(1.0))
            .border_color(Colors::border_subtle())
            .child(fb_button(
                "stem-cancel",
                "Cancel",
                FbButtonKind::Default,
                true,
                {
                    let target = target.clone();
                    move |_, window, cx| {
                        let _ = target.update(cx, |this, cx| this.close(window, cx));
                    }
                },
            ))
            .child(fb_button(
                "stem-extract",
                "Extract",
                FbButtonKind::Primary,
                can_extract,
                {
                    let target = target.clone();
                    move |_, _window, cx| {
                        let _ = target.update(cx, |this, cx| this.start_extract(cx));
                    }
                },
            ))
    }

    fn render_progress(
        &self,
        progress: SphereAudioProcessor::StemExtractProgress,
        target: gpui::Entity<Self>,
    ) -> gpui::AnyElement {
        let percent = format!("{:.0}%", progress.percent);
        let detail = if let Some(stem) = progress.current_stem {
            format!("{} · {}", progress.detail, stem.label())
        } else {
            progress.detail.clone()
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .px(px(BODY_PAD))
            .py(px(BODY_PAD))
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Colors::text_primary())
                            .child(progress.stage.as_str().to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Colors::accent_primary())
                            .child(percent),
                    ),
            )
            .child(progress_bar(ProgressBarValue::value(
                progress.percent / 100.0,
            )))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_muted())
                    .child(detail),
            )
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_faint())
                    .child(format!(
                        "{} · {}",
                        self.params.model.label(),
                        self.params.device.label()
                    )),
            )
            .child(div().flex_1())
            .child(
                div().flex().flex_row().justify_end().child(fb_button(
                    "stem-cancel-run",
                    "Cancel",
                    FbButtonKind::Default,
                    true,
                    {
                        let target = target.clone();
                        move |_, _window, cx| {
                            let _ = target.update(cx, |this, cx| this.request_cancel(cx));
                        }
                    },
                )),
            )
            .into_any_element()
    }

    fn render_complete(
        &self,
        summary: StemExtractJobSummary,
        target: gpui::Entity<Self>,
    ) -> gpui::AnyElement {
        let count = summary.output_paths.len();
        let first = summary
            .output_paths
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let folder = summary
            .output_paths
            .first()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .px(px(BODY_PAD))
            .py(px(BODY_PAD))
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::accent_primary())
                    .child("Extraction complete"),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(Colors::text_primary())
                    .child(format!(
                        "{count} stem file(s) · {} · {}",
                        summary.model.label(),
                        summary.device.label()
                    )),
            )
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_muted())
                    .truncate()
                    .child(first),
            )
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_faint())
                    .child(format!("Backend · {}", summary.backend.label())),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(fb_button(
                        "stem-reveal",
                        "Open Folder",
                        FbButtonKind::Default,
                        folder.is_some(),
                        move |_, _window, _cx| {
                            if let Some(dir) = &folder {
                                let _ = open_in_file_manager(dir);
                            }
                        },
                    ))
                    .child(fb_button(
                        "stem-close",
                        "Close",
                        FbButtonKind::Primary,
                        true,
                        {
                            let target = target.clone();
                            move |_, window, cx| {
                                let _ = target.update(cx, |this, cx| this.close(window, cx));
                            }
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_terminal(
        &self,
        message: &str,
        color: gpui::Rgba,
        target: gpui::Entity<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .px(px(BODY_PAD))
            .py(px(BODY_PAD))
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(color)
                    .child(message.to_string()),
            )
            .child(div().flex_1())
            .child(div().flex().flex_row().justify_end().child(fb_button(
                "stem-close-term",
                "Close",
                FbButtonKind::Primary,
                true,
                {
                    let target = target.clone();
                    move |_, window, cx| {
                        let _ = target.update(cx, |this, cx| this.close(window, cx));
                    }
                },
            )))
            .into_any_element()
    }
}

fn model_hint(params: &SphereAudioProcessor::StemExtractParams) -> String {
    let device = if params.device == SphereAudioProcessor::InferDevice::Gpu {
        if SphereAudioProcessor::gpu_available() {
            "GPU"
        } else if params.allow_cpu_fallback {
            "GPU (falls back to CPU if unavailable)"
        } else {
            "GPU (unavailable)"
        }
    } else {
        "CPU"
    };
    format!(
        "{} · {device} · {}",
        params.model.description(),
        params.quality.label()
    )
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .mt(px(2.0))
        .pb(px(2.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_faint())
        .child(label)
}

fn error_banner(message: String) -> impl IntoElement {
    banner(message, Colors::status_error())
}

fn hint_banner(message: String) -> impl IntoElement {
    banner(message, Colors::text_muted())
}

fn banner(message: String, color: gpui::Rgba) -> impl IntoElement {
    div()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(5.0))
        .bg(Colors::surface_panel_alt())
        .text_size(px(10.0))
        .text_color(color)
        .child(message)
}

fn write_stem_wav(
    path: &std::path::Path,
    stem: &SphereAudioProcessor::StemExtractOutput,
) -> Result<(), String> {
    let channels = stem.channels.max(1) as u16;
    let spec = AudioEncodeSpec {
        sample_rate: stem.sample_rate,
        channels,
        sample_format: AudioSampleFormat::F32,
    };
    let options = AudioEncodeOptions {
        format: AudioFileFormat::Wav,
        ..AudioEncodeOptions::default()
    };
    let mut encoder = create_encoder(path, spec, options).map_err(|e| e.to_string())?;
    encoder
        .write_interleaved_f32(&stem.samples)
        .map_err(|e| e.to_string())?;
    encoder.finalize().map_err(|e| e.to_string())?;
    Ok(())
}

fn sanitize_file_stem(name: &str) -> String {
    let sanitized: String = name
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect();
    if sanitized.is_empty() {
        "stem".to_string()
    } else {
        sanitized
    }
}

fn open_in_file_manager(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(dir).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(dir).spawn()?;
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(dir).spawn()?;
    }
    Ok(())
}

/// Open the Stem Extractor dialog centered over `owner_bounds`.
pub fn open_stem_extractor_window(
    owner_bounds: Option<Bounds<gpui::Pixels>>,
    defaults: StemExtractorDialogDefaults,
    cx: &mut App,
) -> Result<WindowHandle<StemExtractorWindow>, String> {
    let height = TITLEBAR_HEIGHT + STEM_EXTRACTOR_WINDOW_HEIGHT;
    let window_bounds = centered_window_bounds(
        owner_bounds,
        size(px(STEM_EXTRACTOR_WINDOW_WIDTH), px(height)),
        cx,
    );

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(window_bounds));
    options.kind = WindowKind::Dialog;
    options.is_resizable = false;
    options.is_minimizable = false;
    options.window_background = WindowBackgroundAppearance::Transparent;
    apply_owner_display(&mut options, owner_bounds, cx);

    cx.open_window(options, move |_window, cx| {
        cx.new(|cx| StemExtractorWindow::new(defaults, cx))
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_illegal_path_chars() {
        assert_eq!(sanitize_file_stem("Lead/Vox:A"), "Lead_Vox_A");
        assert_eq!(sanitize_file_stem("   "), "stem");
    }
}
