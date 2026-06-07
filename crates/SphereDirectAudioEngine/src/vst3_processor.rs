use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_float};
use std::sync::Arc;

use serde_json::Value;

/// `FUTUREBOARD_VST3_MIDI_DEBUG=1` enables VST3 MIDI bridge traces.
pub fn vst3_midi_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_VST3_MIDI_DEBUG").is_some())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vst3MidiEventKind {
    NoteOff = 0,
    NoteOn = 1,
    /// MIDI controller change. `pitch` carries the VST3 controller number
    /// (`0..=127` CC, `128` aftertouch, `129` pitch bend) and `velocity` the
    /// normalized value `0.0..=1.0`. The C++ bridge maps it to a parameter
    /// change via `IMidiMapping`.
    ControlChange = 2,
}

/// C-compatible MIDI event for `sphere_daux_vst3_process_stereo_block_with_midi`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vst3MidiEvent {
    pub sample_offset: u32,
    pub kind: u8,
    pub channel: u8,
    pub pitch: u8,
    pub velocity: f32,
}

impl Vst3MidiEvent {
    #[inline]
    pub fn note_on(sample_offset: u32, channel: u8, pitch: u8, velocity: f32) -> Self {
        Self {
            sample_offset,
            kind: Vst3MidiEventKind::NoteOn as u8,
            channel,
            pitch,
            velocity,
        }
    }

    #[inline]
    pub fn note_off(sample_offset: u32, channel: u8, pitch: u8, velocity: f32) -> Self {
        Self {
            sample_offset,
            kind: Vst3MidiEventKind::NoteOff as u8,
            channel,
            pitch,
            velocity,
        }
    }

    /// A controller change. `controller` is the VST3 controller number; values
    /// `0..=129` fit in the `pitch` byte. `value` is normalized `0.0..=1.0`.
    #[inline]
    pub fn control_change(sample_offset: u32, channel: u8, controller: u16, value: f32) -> Self {
        Self {
            sample_offset,
            kind: Vst3MidiEventKind::ControlChange as u8,
            channel,
            pitch: controller as u8,
            velocity: value,
        }
    }
}

#[repr(C)]
struct SphereDauxVst3Processor {
    _private: [u8; 0],
}

extern "C" {
    fn sphere_daux_vst3_bridge_probe() -> i32;
    fn sphere_daux_vst3_last_error() -> *const c_char;
    fn sphere_daux_vst3_create(
        plugin_path: *const c_char,
        class_id: *const c_char,
        sample_rate: c_double,
    ) -> *mut SphereDauxVst3Processor;
    fn sphere_daux_vst3_destroy(processor: *mut SphereDauxVst3Processor);
    fn sphere_daux_vst3_process_stereo_sample(
        processor: *mut SphereDauxVst3Processor,
        in_l: c_float,
        in_r: c_float,
        out_l: *mut c_float,
        out_r: *mut c_float,
    ) -> i32;
    #[allow(dead_code)]
    fn sphere_daux_vst3_process_stereo_block(
        processor: *mut SphereDauxVst3Processor,
        in_l: *const c_float,
        in_r: *const c_float,
        out_l: *mut c_float,
        out_r: *mut c_float,
        frames: i32,
    ) -> i32;
    fn sphere_daux_vst3_process_stereo_block_with_midi(
        processor: *mut SphereDauxVst3Processor,
        in_l: *const c_float,
        in_r: *const c_float,
        out_l: *mut c_float,
        out_r: *mut c_float,
        frames: i32,
        events: *const Vst3MidiEvent,
        event_count: i32,
    ) -> i32;
    fn sphere_daux_vst3_event_input_bus_count(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_process_count(processor: *mut SphereDauxVst3Processor) -> u64;
    fn sphere_daux_vst3_last_input_peak(processor: *mut SphereDauxVst3Processor) -> c_double;
    fn sphere_daux_vst3_last_output_peak(processor: *mut SphereDauxVst3Processor) -> c_double;
    fn sphere_daux_vst3_last_difference_peak(processor: *mut SphereDauxVst3Processor) -> c_double;
    /// Enqueue a normalized (0..1) VST3 parameter change.
    /// Delivered to IAudioProcessor via inputParameterChanges on the next process call.
    fn sphere_daux_vst3_set_param(
        processor: *mut SphereDauxVst3Processor,
        param_id: u32,
        value: c_double,
    );
    fn sphere_daux_vst3_open_editor(
        processor: *mut SphereDauxVst3Processor,
        window_id: *const c_char,
        title: *const c_char,
        width: i32,
        height: i32,
    ) -> u64;
    fn sphere_daux_vst3_close_editor(processor: *mut SphereDauxVst3Processor);
    fn sphere_daux_vst3_focus_editor(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_is_valid(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_get_latency_samples(processor: *mut SphereDauxVst3Processor) -> i32;
    // GPUI-embedded editor (Windows): attaches the existing instance's view.
    fn sphere_daux_vst3_embed_editor(
        processor: *mut SphereDauxVst3Processor,
        parent_hwnd: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> u64;
    fn sphere_daux_vst3_embed_set_bounds(
        processor: *mut SphereDauxVst3Processor,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    );
    fn sphere_daux_vst3_embed_refresh(processor: *mut SphereDauxVst3Processor);
    fn sphere_daux_vst3_embed_detach(processor: *mut SphereDauxVst3Processor);
    fn sphere_daux_vst3_embed_is_valid(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_embed_has_visible_ui(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_embed_host_kind(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_embed_take_user_close(processor: *mut SphereDauxVst3Processor) -> i32;
    fn sphere_daux_vst3_embed_content_size(
        processor: *mut SphereDauxVst3Processor,
        out_width: *mut i32,
        out_height: *mut i32,
    ) -> i32;
}

#[derive(Debug)]
pub struct Vst3RuntimeProcessor {
    inner: Arc<Vst3RuntimeProcessorInner>,
}

#[derive(Debug)]
struct Vst3RuntimeProcessorInner {
    raw: *mut SphereDauxVst3Processor,
    plugin_path: String,
    class_id: String,
    sample_rate: u32,
    event_input_bus_count: i32,
    destroy_reason: std::sync::Mutex<Option<String>>,
}

unsafe impl Send for Vst3RuntimeProcessor {}
unsafe impl Sync for Vst3RuntimeProcessor {}
unsafe impl Send for Vst3RuntimeProcessorInner {}
unsafe impl Sync for Vst3RuntimeProcessorInner {}

impl Vst3RuntimeProcessor {
    pub fn from_params(
        params: &std::collections::HashMap<String, Value>,
        sample_rate: u32,
    ) -> Option<Self> {
        let plugin_path = params
            .get("modulePath")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str)?
            .trim();
        if plugin_path.is_empty() {
            return None;
        }
        let class_id = params
            .get("classId")
            .or_else(|| params.get("class_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        Self::new(plugin_path, class_id, sample_rate)
    }

    pub fn new(plugin_path: &str, class_id: &str, sample_rate: u32) -> Option<Self> {
        let bridge_probe = unsafe { sphere_daux_vst3_bridge_probe() };
        eprintln!("[SphereVST3] bridge probe result=0x{bridge_probe:x}");
        eprintln!(
            "[SphereVST3] create request path='{}' exists={} classId='{}' sr={}",
            plugin_path,
            std::path::Path::new(plugin_path).exists(),
            class_id,
            sample_rate.max(1)
        );
        let path = CString::new(plugin_path).ok()?;
        let class_id_c = CString::new(class_id).ok()?;
        let raw = unsafe {
            sphere_daux_vst3_create(
                path.as_ptr(),
                class_id_c.as_ptr(),
                sample_rate.max(1) as c_double,
            )
        };
        if raw.is_null() {
            let reason = unsafe {
                let ptr = sphere_daux_vst3_last_error();
                if ptr.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(ptr).to_string_lossy().into_owned()
                }
            };
            eprintln!(
                "[SphereVST3] create failed path='{}' classId='{}' reason={}",
                plugin_path, class_id, reason
            );
            return None;
        }
        let event_input_bus_count = unsafe { sphere_daux_vst3_event_input_bus_count(raw) };
        eprintln!(
            "[SphereVST3] create ok path='{}' classId='{}' handle=0x{:x} eventInputBuses={}",
            plugin_path, class_id, raw as usize, event_input_bus_count
        );
        Some(Self {
            inner: Arc::new(Vst3RuntimeProcessorInner {
                raw,
                plugin_path: plugin_path.to_string(),
                class_id: class_id.to_string(),
                sample_rate: sample_rate.max(1),
                event_input_bus_count,
                destroy_reason: std::sync::Mutex::new(None),
            }),
        })
    }

    #[inline]
    pub fn process_stereo_sample(&mut self, l: f32, r: f32) -> Option<(f32, f32)> {
        if self.inner.raw.is_null() {
            return None;
        }
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        let ok = unsafe {
            sphere_daux_vst3_process_stereo_sample(self.inner.raw, l, r, &mut out_l, &mut out_r)
        };
        if ok == 0 {
            None
        } else {
            Some((out_l, out_r))
        }
    }

    #[inline]
    pub fn process_stereo_block(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) -> bool {
        self.process_stereo_block_with_midi(in_l, in_r, out_l, out_r, &[])
    }

    #[inline]
    pub fn event_input_bus_count(&self) -> i32 {
        self.inner.event_input_bus_count
    }

    #[inline]
    pub fn process_stereo_block_with_midi(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        midi_events: &[Vst3MidiEvent],
    ) -> bool {
        let frames = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len());
        if self.inner.raw.is_null() || frames == 0 {
            return false;
        }
        let (events_ptr, event_count) = if midi_events.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (midi_events.as_ptr(), midi_events.len() as i32)
        };
        let ok = unsafe {
            sphere_daux_vst3_process_stereo_block_with_midi(
                self.inner.raw,
                in_l.as_ptr(),
                in_r.as_ptr(),
                out_l.as_mut_ptr(),
                out_r.as_mut_ptr(),
                frames as i32,
                events_ptr,
                event_count,
            )
        };
        ok != 0
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        !self.inner.raw.is_null()
    }

    #[inline]
    pub fn last_error(&self) -> Option<String> {
        if self.inner.raw.is_null() {
            return None;
        }
        unsafe {
            let ptr = sphere_daux_vst3_last_error();
            if ptr.is_null() {
                None
            } else {
                let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            }
        }
    }

    #[inline]
    pub fn plugin_path(&self) -> Option<&str> {
        if self.inner.plugin_path.is_empty() {
            None
        } else {
            Some(&self.inner.plugin_path)
        }
    }

    #[inline]
    pub fn class_id(&self) -> Option<&str> {
        if self.inner.class_id.is_empty() {
            None
        } else {
            Some(&self.inner.class_id)
        }
    }

    #[inline]
    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate
    }

    #[inline]
    pub fn handle_value(&self) -> usize {
        self.inner.raw as usize
    }

    #[inline]
    pub fn process_count(&self) -> u64 {
        if self.inner.raw.is_null() {
            0
        } else {
            unsafe { sphere_daux_vst3_process_count(self.inner.raw) }
        }
    }

    #[inline]
    pub fn last_input_peak(&self) -> f64 {
        if self.inner.raw.is_null() {
            0.0
        } else {
            unsafe { sphere_daux_vst3_last_input_peak(self.inner.raw) as f64 }
        }
    }

    #[inline]
    pub fn last_output_peak(&self) -> f64 {
        if self.inner.raw.is_null() {
            0.0
        } else {
            unsafe { sphere_daux_vst3_last_output_peak(self.inner.raw) as f64 }
        }
    }

    #[inline]
    pub fn last_difference_peak(&self) -> f64 {
        if self.inner.raw.is_null() {
            0.0
        } else {
            unsafe { sphere_daux_vst3_last_difference_peak(self.inner.raw) as f64 }
        }
    }

    /// Enqueue a normalized (0..1) parameter change for the given VST3 ParamID.
    ///
    /// The change is delivered to `IAudioProcessor` via `inputParameterChanges`
    /// on the next `process_stereo_sample` call.  Safe to call from the audio
    /// thread (inside command-drain) or from any other thread.
    ///
    /// `param_id` — the integer `Steinberg::Vst::ParamID` as exposed by the plugin.
    /// `value`    — normalized value in `[0.0, 1.0]`.
    #[inline]
    pub fn set_param(&mut self, param_id: u32, value: f64) {
        if self.inner.raw.is_null() {
            return;
        }
        unsafe { sphere_daux_vst3_set_param(self.inner.raw, param_id, value as c_double) }
    }

    /// Mark this processor for destruction with a reason string logged at drop time.
    /// Call this before removing the insert so the drop log is meaningful.
    pub fn set_destroy_reason(&self, reason: &str) {
        if let Ok(mut guard) = self.inner.destroy_reason.lock() {
            *guard = Some(reason.to_string());
        }
    }

    /// Returns false if the underlying C++ processor has been destroyed.
    /// The audio callback should bypass the insert if this returns false.
    #[inline]
    pub fn is_processor_valid(&self) -> bool {
        if self.inner.raw.is_null() {
            return false;
        }
        unsafe { sphere_daux_vst3_is_valid(self.inner.raw) != 0 }
    }

    /// Returns the plugin's reported latency in samples.
    /// Divide by `sample_rate()` to convert to seconds.
    #[inline]
    pub fn get_latency_samples(&self) -> i32 {
        if self.inner.raw.is_null() {
            return 0;
        }
        unsafe { sphere_daux_vst3_get_latency_samples(self.inner.raw) }
    }

    pub fn open_editor(
        &mut self,
        window_id: &str,
        title: &str,
        width: i32,
        height: i32,
    ) -> Option<u64> {
        if self.inner.raw.is_null() {
            return None;
        }
        let window_id = CString::new(window_id).ok()?;
        let title = CString::new(title).ok()?;
        let handle = unsafe {
            sphere_daux_vst3_open_editor(
                self.inner.raw,
                window_id.as_ptr(),
                title.as_ptr(),
                width,
                height,
            )
        };
        if handle == 0 {
            None
        } else {
            Some(handle)
        }
    }

    pub fn close_editor(&mut self) {
        if self.inner.raw.is_null() {
            return;
        }
        unsafe { sphere_daux_vst3_close_editor(self.inner.raw) };
    }

    pub fn focus_editor(&mut self) -> bool {
        if self.inner.raw.is_null() {
            return false;
        }
        unsafe { sphere_daux_vst3_focus_editor(self.inner.raw) != 0 }
    }

    // ── GPUI-embedded editor ──────────────────────────────────────────────
    //
    // Attach THIS runtime instance's editor view into a GPUI-provided parent
    // window. No new VST3 component/controller is created; GUI parameter edits
    // affect the live audio processor. These take `&self` because they only
    // pass the opaque processor pointer across the FFI — no Rust-side mutation.

    /// Attach the editor view into `parent_hwnd` at the given physical-pixel
    /// region (parent-client coords). Returns a non-zero editor handle, or
    /// `None` on failure.
    pub fn embed_editor(
        &self,
        parent_hwnd: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Option<u64> {
        if self.inner.raw.is_null() {
            return None;
        }
        let handle = unsafe {
            sphere_daux_vst3_embed_editor(self.inner.raw, parent_hwnd, x, y, width, height)
        };
        if handle == 0 {
            None
        } else {
            Some(handle)
        }
    }

    pub fn embed_set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        if self.inner.raw.is_null() {
            return;
        }
        unsafe { sphere_daux_vst3_embed_set_bounds(self.inner.raw, x, y, width, height) };
    }

    pub fn embed_refresh(&self) {
        if self.inner.raw.is_null() {
            return;
        }
        unsafe { sphere_daux_vst3_embed_refresh(self.inner.raw) };
    }

    /// Detach the embedded view and destroy the host window. The processor
    /// (and audio) keep running.
    pub fn embed_detach(&self) {
        if self.inner.raw.is_null() {
            return;
        }
        unsafe { sphere_daux_vst3_embed_detach(self.inner.raw) };
    }

    pub fn embed_is_valid(&self) -> bool {
        if self.inner.raw.is_null() {
            return false;
        }
        unsafe { sphere_daux_vst3_embed_is_valid(self.inner.raw) != 0 }
    }

    pub fn embed_has_visible_ui(&self) -> bool {
        if self.inner.raw.is_null() {
            return false;
        }
        unsafe { sphere_daux_vst3_embed_has_visible_ui(self.inner.raw) != 0 }
    }

    /// 0 = WS_CHILD, 1 = owned tool window, 2 = detached top-level, -1 = none.
    pub fn embed_host_kind(&self) -> i32 {
        if self.inner.raw.is_null() {
            return -1;
        }
        unsafe { sphere_daux_vst3_embed_host_kind(self.inner.raw) }
    }

    /// Detached mode only: `true` (and resets) when the user closed the
    /// standalone editor window, so the host can tear the editor shell down.
    pub fn embed_take_user_close(&self) -> bool {
        if self.inner.raw.is_null() {
            return false;
        }
        unsafe { sphere_daux_vst3_embed_take_user_close(self.inner.raw) != 0 }
    }

    pub fn embed_content_size(&self) -> Option<(i32, i32)> {
        if self.inner.raw.is_null() {
            return None;
        }
        let mut width = 0;
        let mut height = 0;
        let ok =
            unsafe { sphere_daux_vst3_embed_content_size(self.inner.raw, &mut width, &mut height) };
        if ok != 0 && width > 0 && height > 0 {
            Some((width, height))
        } else {
            None
        }
    }
}

impl Clone for Vst3RuntimeProcessor {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for Vst3RuntimeProcessorInner {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }
        let reason = self
            .destroy_reason
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| "unknown".to_string());
        eprintln!(
            "[SphereVST3] destroying shared processor path='{}' classId='{}' sr={} reason={}",
            self.plugin_path, self.class_id, self.sample_rate, reason
        );
        unsafe { sphere_daux_vst3_destroy(self.raw) };
        self.raw = std::ptr::null_mut();
        eprintln!(
            "[SphereVST3] destroyed shared processor path='{}' classId='{}' sr={} reason={}",
            self.plugin_path, self.class_id, self.sample_rate, reason
        );
    }
}
