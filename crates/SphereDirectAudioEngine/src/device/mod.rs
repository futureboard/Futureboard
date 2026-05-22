use crate::types::JsAudioDeviceInfo;
use cpal::traits::{DeviceTrait, HostTrait};

/// Enumerate all available output devices on the default host.
/// Never panics — returns empty vec on any cpal error.
pub fn list_output_devices() -> Vec<JsAudioDeviceInfo> {
    let host = cpal::default_host();
    let backend = host.id().name().to_string();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());

    match host.output_devices() {
        Err(e) => {
            eprintln!("[SphereAudio] list_output_devices error: {e}");
            vec![]
        }
        Ok(devices) => devices
            .filter_map(|dev| {
                let name = dev.name().ok()?;
                let cfg = dev.default_output_config().ok()?;
                Some(JsAudioDeviceInfo {
                    id: name.clone(),
                    name: name.clone(),
                    kind: "output".into(),
                    channels: cfg.channels() as u32,
                    default_sample_rate: cfg.sample_rate().0,
                    is_default: Some(&name) == default_name.as_ref(),
                    backend: backend.clone(),
                })
            })
            .collect(),
    }
}

/// Enumerate all available input devices on the default host.
pub fn list_input_devices() -> Vec<JsAudioDeviceInfo> {
    let host = cpal::default_host();
    let backend = host.id().name().to_string();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    match host.input_devices() {
        Err(e) => {
            eprintln!("[SphereAudio] list_input_devices error: {e}");
            vec![]
        }
        Ok(devices) => devices
            .filter_map(|dev| {
                let name = dev.name().ok()?;
                let cfg = dev.default_input_config().ok()?;
                Some(JsAudioDeviceInfo {
                    id: name.clone(),
                    name: name.clone(),
                    kind: "input".into(),
                    channels: cfg.channels() as u32,
                    default_sample_rate: cfg.sample_rate().0,
                    is_default: Some(&name) == default_name.as_ref(),
                    backend: backend.clone(),
                })
            })
            .collect(),
    }
}

/// Resolve a named output device (or the system default if `id` is None).
/// Returns `(device, actual_name)` or an error string.
pub fn resolve_output_device(id: Option<&str>) -> Result<(cpal::Device, String), String> {
    let host = cpal::default_host();
    match id {
        None => {
            let dev = host
                .default_output_device()
                .ok_or_else(|| "No default output device found".to_string())?;
            let name = dev.name().unwrap_or_else(|_| "Unknown".into());
            Ok((dev, name))
        }
        Some(wanted) => {
            let mut devices = host.output_devices().map_err(|e| e.to_string())?;
            devices
                .find(|d| d.name().map(|n| n == wanted).unwrap_or(false))
                .map(|d| {
                    let n = d.name().unwrap_or_else(|_| wanted.into());
                    (d, n)
                })
                .ok_or_else(|| format!("Output device '{wanted}' not found"))
        }
    }
}
