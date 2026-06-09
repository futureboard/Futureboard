use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use sphere_plugin_host::ipc::{HostCommand, HostEvent};
use sphere_plugin_host::plugin_host_client::{
    plugin_host_bridge_enabled, ClientEvent, PluginHostClient, PluginHostClientError,
};
use sphere_plugin_host::plugin_host_lifecycle::{self, BridgeHostManager};

#[derive(Debug, Clone)]
pub(crate) struct BridgePluginDescriptor {
    pub track_id: String,
    pub insert_id: String,
    pub plugin_path: String,
    pub class_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeLoadedPlugin {
    pub descriptor: BridgePluginDescriptor,
    pub host_pid: Option<u32>,
}

pub(crate) struct PluginBridgeRuntime {
    client: PluginHostClient,
    host_pid: Option<u32>,
    loaded: HashMap<String, BridgeLoadedPlugin>,
    queued_events: VecDeque<ClientEvent>,
    /// Stage 1: the engine-owned (sample_rate, block) most recently pushed to the
    /// host via `ConfigureAudioBridge`. `None` until the first configure so the
    /// next `LoadPlugin` sends it first.
    audio_bridge_config: Option<(u32, u32)>,
    /// Stage 2: the engine-created shared-memory audio region, kept mapped for
    /// the host's lifetime. `Arc` so a Stage-3b realtime sink can share it with
    /// the audio engine. `None` until the first `AttachSharedAudio` succeeds.
    shared_audio: Option<Arc<sphere_plugin_host::audio_bridge::SharedAudioRegion>>,
}

pub(crate) type SharedPluginBridgeRuntime = Arc<Mutex<PluginBridgeRuntime>>;

pub(super) fn bridge_enabled() -> bool {
    plugin_host_bridge_enabled()
}

pub(super) fn legacy_in_process_enabled() -> bool {
    sphere_plugin_host::plugin_host_client::legacy_in_process_enabled()
}

impl PluginBridgeRuntime {
    pub fn ensure_shared(
        slot: &mut Option<SharedPluginBridgeRuntime>,
    ) -> Result<SharedPluginBridgeRuntime, PluginHostClientError> {
        if let Some(existing) = slot.as_ref() {
            return Ok(existing.clone());
        }
        eprintln!("[plugin-bridge] ensure_host running=false -> spawn");
        let mut client = PluginHostClient::spawn_bridge()?;
        let host_pid = Some(client.pid());
        // The host emits Ready on startup; retain it for any caller that wants
        // to poll, but do not block insert on a second handshake.
        let _ = client.ping();
        let runtime = Arc::new(Mutex::new(Self {
            client,
            host_pid,
            loaded: HashMap::new(),
            queued_events: VecDeque::new(),
            audio_bridge_config: None,
            shared_audio: None,
        }));
        *slot = Some(runtime.clone());
        Ok(runtime)
    }

    pub fn host_pid(&self) -> Option<u32> {
        self.host_pid
    }

    /// Stage 3b: a realtime sink over the shared audio region for the engine to
    /// mix plugin-host DSP output into the master. `None` until the region is
    /// established (and on non-Windows, where the mapping is unavailable).
    pub fn audio_sink(&self) -> Option<DAUx::plugin_bridge::SharedPluginBridgeSink> {
        let region = self.shared_audio.as_ref()?;
        Some(sphere_plugin_host::plugin_bridge_sink::SharedRegionSink::into_shared(region.clone()))
    }

    pub fn loaded_descriptor(&self, instance: &str) -> Option<BridgeLoadedPlugin> {
        self.loaded.get(instance).cloned()
    }

    pub fn loaded_for_track(&self, track_id: &str) -> Option<BridgeLoadedPlugin> {
        self.loaded
            .values()
            .find(|loaded| loaded.descriptor.track_id == track_id)
            .cloned()
    }

    /// Stage 1: push the engine-owned sample rate / block size to the host so it
    /// follows them for plugin DSP. Idempotent — only re-sent when the config
    /// actually changes. The host replies `AudioBridgeConfigured`.
    pub fn configure_audio_bridge(
        &mut self,
        sample_rate: u32,
        max_block_size: u32,
    ) -> Result<(), PluginHostClientError> {
        if self.audio_bridge_config == Some((sample_rate, max_block_size)) {
            return Ok(());
        }
        eprintln!(
            "[plugin-bridge] sending ConfigureAudioBridge sample_rate={sample_rate} max_block_size={max_block_size} (engine owns)"
        );
        self.client
            .configure_audio_bridge(sample_rate, max_block_size)?;
        self.audio_bridge_config = Some((sample_rate, max_block_size));
        // Stage 2: stand up the shared-memory audio region and map it in the host.
        self.establish_shared_audio(sample_rate, max_block_size);
        Ok(())
    }

    /// Stage 2: create the engine-owned named shared-memory region (once) and
    /// ask the host to map it. Idempotent; logs + retains the region so the
    /// mapping stays alive. No-op on non-Windows (mapping is Windows-only here).
    fn establish_shared_audio(&mut self, sample_rate: u32, max_block_size: u32) {
        if self.shared_audio.is_some() {
            return;
        }
        #[cfg(windows)]
        {
            use sphere_plugin_host::audio_bridge::SharedAudioRegion;
            let name = format!("Local\\FutureboardAudioBridge-{}", std::process::id());
            match SharedAudioRegion::create_named(&name, sample_rate, max_block_size, 2) {
                Ok(region) => {
                    let bytes = region.bytes();
                    eprintln!(
                        "[plugin-bridge] shared audio region created name={name} bytes={bytes} sr={sample_rate} block={max_block_size}"
                    );
                    eprintln!(
                        "[plugin-bridge] sending AttachSharedAudio name={name} bytes={bytes}"
                    );
                    match self.client.attach_shared_audio(name.clone(), bytes) {
                        Ok(()) => self.shared_audio = Some(Arc::new(region)),
                        Err(error) => {
                            eprintln!("[plugin-bridge] AttachSharedAudio send failed: {error}")
                        }
                    }
                }
                Err(error) => {
                    eprintln!("[plugin-bridge] shared audio region create failed: {error}")
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (sample_rate, max_block_size);
        }
    }

    pub fn send_load_plugin(
        &mut self,
        descriptor: BridgePluginDescriptor,
        sample_rate: u32,
        max_block_size: u32,
    ) -> Result<(), PluginHostClientError> {
        // Stage 1: the engine owns SR/block — make sure the host is following
        // them before the first plugin loads.
        let _ = self.configure_audio_bridge(sample_rate, max_block_size);
        let instance = descriptor.insert_id.clone();
        eprintln!(
            "[plugin-bridge] sending LoadPlugin instance={} path={}",
            instance, descriptor.plugin_path
        );
        self.loaded.insert(
            instance.clone(),
            BridgeLoadedPlugin {
                descriptor: descriptor.clone(),
                host_pid: self.host_pid,
            },
        );
        self.client.load_plugin(
            instance.clone(),
            descriptor.plugin_path,
            descriptor.class_id,
            sample_rate,
            max_block_size,
        )?;
        let input_channels = 0u32;
        let output_channels = 2u32;
        eprintln!(
            "[plugin-bridge] sending PrepareProcessing instance={instance} sr={sample_rate} block={max_block_size}"
        );
        self.client.prepare_processing(
            instance,
            sample_rate,
            max_block_size,
            input_channels,
            output_channels,
        )
    }

    pub fn open_editor_with_parent(
        &mut self,
        plugin_instance_id: String,
        parent_hwnd: u64,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> Result<(), PluginHostClientError> {
        let (path, class_id) = self
            .loaded
            .get(&plugin_instance_id)
            .map(|plugin| {
                (
                    plugin.descriptor.plugin_path.clone(),
                    plugin.descriptor.class_id.clone(),
                )
            })
            .unwrap_or_else(|| (String::new(), String::new()));
        eprintln!("[plugin-bridge] OpenEditorWithParentHwnd hwnd=0x{parent_hwnd:x}");
        self.client.open_editor(
            plugin_instance_id,
            path,
            class_id,
            parent_hwnd,
            width,
            height,
            dpi,
        )
    }

    pub fn prepare_editor_view(
        &mut self,
        plugin_instance_id: String,
    ) -> Result<(), PluginHostClientError> {
        let (path, class_id) = self
            .loaded
            .get(&plugin_instance_id)
            .map(|plugin| {
                (
                    plugin.descriptor.plugin_path.clone(),
                    plugin.descriptor.class_id.clone(),
                )
            })
            .unwrap_or_else(|| (String::new(), String::new()));
        eprintln!("[plugin-bridge] PrepareEditorView instance={plugin_instance_id}");
        self.client
            .prepare_editor_view(plugin_instance_id, path, class_id)
    }

    pub fn confirm_editor_content_ready(
        &mut self,
        plugin_instance_id: String,
        parent_hwnd: u64,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> Result<(), PluginHostClientError> {
        eprintln!(
            "[plugin-bridge] ConfirmEditorContentReady instance={plugin_instance_id} hwnd=0x{parent_hwnd:x} size={width}x{height}"
        );
        self.client.confirm_editor_content_ready(
            plugin_instance_id,
            parent_hwnd,
            width,
            height,
            dpi,
        )
    }

    pub fn preview_note_on(
        &mut self,
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    ) -> Result<(), PluginHostClientError> {
        eprintln!(
            "[plugin-bridge] sending PreviewNoteOn instance={plugin_instance_id} ch={channel} pitch={pitch} vel={velocity}"
        );
        self.client
            .preview_note_on(plugin_instance_id, channel, pitch, velocity)
    }

    pub fn preview_note_off(
        &mut self,
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
    ) -> Result<(), PluginHostClientError> {
        eprintln!(
            "[plugin-bridge] sending PreviewNoteOff instance={plugin_instance_id} ch={channel} pitch={pitch}"
        );
        self.client
            .preview_note_off(plugin_instance_id, channel, pitch)
    }

    pub fn preview_all_notes_off(
        &mut self,
        plugin_instance_id: String,
    ) -> Result<(), PluginHostClientError> {
        eprintln!("[plugin-bridge] sending PreviewAllNotesOff instance={plugin_instance_id}");
        self.client.preview_all_notes_off(plugin_instance_id)
    }

    pub fn midi_panic(&mut self, plugin_instance_id: String) -> Result<(), PluginHostClientError> {
        eprintln!("[plugin-bridge] sending MidiPanic instance={plugin_instance_id}");
        self.client.midi_panic(plugin_instance_id)
    }

    pub fn resize_editor(&mut self, plugin_instance_id: String, width: u32, height: u32, dpi: u32) {
        let _ = self
            .client
            .resize_editor(plugin_instance_id, width, height, dpi);
    }

    pub fn close_editor(&mut self, plugin_instance_id: String) {
        eprintln!("[plugin-bridge] CloseEditor instance={plugin_instance_id}");
        let _ = self.client.close_editor(plugin_instance_id);
    }

    pub fn unload_plugin(&mut self, plugin_instance_id: String) {
        eprintln!("[plugin-bridge] UnloadPlugin instance={plugin_instance_id}");
        let _ = self.client.unload_plugin(plugin_instance_id.clone());
        self.loaded.remove(&plugin_instance_id);
    }

    pub fn poll(&mut self) {
        while let Some(event) = self.client.try_recv_event() {
            match &event {
                ClientEvent::Host(HostEvent::Ready { pid, .. })
                | ClientEvent::Host(HostEvent::Pong { pid }) => {
                    self.host_pid = Some(*pid);
                }
                _ => {}
            }
            self.queued_events.push_back(event);
        }
    }

    pub fn drain_events(&mut self) -> Vec<ClientEvent> {
        self.poll();
        self.queued_events.drain(..).collect()
    }

    pub fn send_raw(&mut self, command: &HostCommand) -> Result<(), PluginHostClientError> {
        self.client.send(command)
    }

    /// Graceful shutdown of the shared bridge host. Drains the runtime slot so
    /// the [`PluginHostClient`] is dropped and process handles are released.
    pub fn shutdown_shared(slot: &mut Option<SharedPluginBridgeRuntime>) {
        let host_count = BridgeHostManager::global().host_count();
        eprintln!("[plugin-bridge] shutdown begin hosts={host_count}");
        let Some(runtime) = slot.take() else {
            eprintln!("[plugin-bridge] shutdown complete");
            return;
        };
        if let Ok(mut bridge) = runtime.lock() {
            plugin_host_lifecycle::shutdown_host_client(&mut bridge.client);
            bridge.client.join_reader();
            bridge.loaded.clear();
            bridge.queued_events.clear();
            bridge.host_pid = None;
        }
        drop(runtime);
        BridgeHostManager::global().clear_hosts();
        eprintln!("[plugin-bridge] shutdown complete");
    }
}

/// Shut down every plugin-host child owned by the studio layout.
pub(crate) fn shutdown_plugin_bridge(slot: &mut Option<SharedPluginBridgeRuntime>) {
    PluginBridgeRuntime::shutdown_shared(slot);
}
