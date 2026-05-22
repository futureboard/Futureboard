use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_float};
use std::sync::Arc;

use serde_json::Value;

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
    fn sphere_daux_vst3_process_stereo_block(
        processor: *mut SphereDauxVst3Processor,
        in_l: *const c_float,
        in_r: *const c_float,
        out_l: *mut c_float,
        out_r: *mut c_float,
        frames: i32,
    ) -> i32;
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
        eprintln!(
            "[SphereVST3] create ok path='{}' classId='{}' handle=0x{:x}",
            plugin_path, class_id, raw as usize
        );
        Some(Self {
            inner: Arc::new(Vst3RuntimeProcessorInner {
                raw,
                plugin_path: plugin_path.to_string(),
                class_id: class_id.to_string(),
                sample_rate: sample_rate.max(1),
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
        let frames = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len());
        if self.inner.raw.is_null() || frames == 0 {
            return false;
        }
        let ok = unsafe {
            sphere_daux_vst3_process_stereo_block(
                self.inner.raw,
                in_l.as_ptr(),
                in_r.as_ptr(),
                out_l.as_mut_ptr(),
                out_r.as_mut_ptr(),
                frames as i32,
            )
        };
        ok != 0
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        !self.inner.raw.is_null()
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
