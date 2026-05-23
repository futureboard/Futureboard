use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WindowKind {
    #[serde(alias = "Mixer")]
    Mixer,
    #[serde(alias = "Midi")]
    Midi,
    #[serde(alias = "Analyzer")]
    Analyzer,
    #[serde(rename = "plugin-editor-placeholder", alias = "PluginEditorPlaceholder")]
    PluginEditorPlaceholder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloatingWindowDescriptor {
    pub id: String,
    pub kind: WindowKind,
    pub title: String,
    #[serde(rename = "ownerId", skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(rename = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(
        rename = "initialBounds",
        alias = "initial_bounds",
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_bounds: Option<WindowBounds>,
    #[serde(rename = "alwaysOnTop", alias = "always_on_top", default)]
    pub always_on_top: bool,
    #[serde(rename = "rememberBounds", default)]
    pub remember_bounds: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerTrack {
    pub id: String,
    pub name: String,
    pub color: String,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    #[serde(rename = "meterL", default)]
    pub meter_l: f32,
    #[serde(rename = "meterR", default)]
    pub meter_r: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerMaster {
    pub volume: f32,
    #[serde(rename = "meterL", default)]
    pub meter_l: f32,
    #[serde(rename = "meterR", default)]
    pub meter_r: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiDevice {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(rename = "isInput", default)]
    pub is_input: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiEvent {
    #[serde(rename = "deviceId")]
    pub device_id: String,
    pub channel: u8,
    pub kind: String,
    pub note: Option<u8>,
    pub velocity: Option<u8>,
    pub cc: Option<u8>,
    pub value: Option<u8>,
    pub timestamp: f64,
}

/// Messages received from Electron → floatingwindow (stdin)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
    #[serde(rename = "openWindow")]
    OpenWindow { window: FloatingWindowDescriptor },
    #[serde(rename = "closeWindow")]
    CloseWindow { id: String },
    #[serde(rename = "focusWindow")]
    FocusWindow { id: String },
    #[serde(rename = "mixer:update")]
    MixerUpdate {
        tracks: Vec<MixerTrack>,
        master: Option<MixerMaster>,
    },
    #[serde(rename = "midi:updateDevices")]
    MidiUpdateDevices { devices: Vec<MidiDevice> },
    #[serde(rename = "midi:event")]
    MidiEvent { event: MidiEvent },
}

/// Messages sent from floatingwindow → Electron (stdout)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    #[serde(rename = "windowOpened")]
    WindowOpened { id: String },
    #[serde(rename = "windowClosed")]
    WindowClosed { id: String },
    #[serde(rename = "windowBoundsChanged")]
    WindowBoundsChanged { id: String, bounds: WindowBounds },
    #[serde(rename = "command")]
    Command {
        #[serde(rename = "commandId")]
        command_id: String,
        payload: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_electron_window_descriptor() {
        let json = r#"{
            "type": "openWindow",
            "window": {
                "id": "mixer",
                "kind": "Mixer",
                "title": "Mixer - Futureboard",
                "initial_bounds": { "x": 10, "y": 20, "width": 960, "height": 300 },
                "always_on_top": true
            }
        }"#;

        let msg = serde_json::from_str::<IncomingMessage>(json).unwrap();
        let IncomingMessage::OpenWindow { window } = msg else {
            panic!("expected openWindow");
        };

        assert_eq!(window.kind, WindowKind::Mixer);
        assert_eq!(window.initial_bounds.unwrap().width, 960.0);
        assert!(window.always_on_top);
    }

    #[test]
    fn parses_current_window_descriptor() {
        let json = r#"{
            "type": "openWindow",
            "window": {
                "id": "mixer",
                "kind": "mixer",
                "title": "Mixer - Futureboard",
                "initialBounds": { "x": 10, "y": 20, "width": 960, "height": 300 },
                "alwaysOnTop": true
            }
        }"#;

        let msg = serde_json::from_str::<IncomingMessage>(json).unwrap();
        let IncomingMessage::OpenWindow { window } = msg else {
            panic!("expected openWindow");
        };

        assert_eq!(window.kind, WindowKind::Mixer);
        assert_eq!(window.initial_bounds.unwrap().height, 300.0);
        assert!(window.always_on_top);
    }
}
