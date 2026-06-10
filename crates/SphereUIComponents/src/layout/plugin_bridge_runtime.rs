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
    /// Stage 2: one shared-memory audio region per insert instance. Each region
    /// carries its own `request_seq` / `done_seq` so serial FX chains on one
    /// track do not clobber each other's handshake.
    shared_audio: HashMap<String, Arc<sphere_plugin_host::audio_bridge::SharedAudioRegion>>,
}

pub(crate) type SharedPluginBridgeRuntime = Arc<Mutex<PluginBridgeRuntime>>;

pub(super) fn bridge_enabled() -> bool {
    plugin_host_bridge_enabled()
}

pub(super) fn legacy_in_process_enabled() -> bool {
    sphere_plugin_host::plugin_host_client::legacy_in_process_enabled()
}

/// Named shared-memory region for one insert instance.
pub(crate) fn bridge_region_name(instance_id: &str) -> String {
    format!(
        "Local\\FutureboardAudioBridge-{}__{}",
        std::process::id(),
        instance_id
    )
}

#[cfg(test)]
mod tests {
    use super::bridge_region_name;

    #[test]
    fn bridge_region_names_are_unique_per_insert_instance() {
        let a = bridge_region_name("insert-track1-1");
        let b = bridge_region_name("insert-track1-2");
        assert_ne!(a, b, "each insert instance must get its own shared region name");
        assert!(a.contains("insert-track1-1"));
        assert!(b.contains("insert-track1-2"));
    }
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
            shared_audio: HashMap::new(),
        }));
        *slot = Some(runtime.clone());
        Ok(runtime)
    }

    pub fn host_pid(&self) -> Option<u32> {
        self.host_pid
    }

    /// Stage 3b: realtime sink for one insert instance.
    pub fn audio_sink_for(
        &self,
        instance_id: &str,
    ) -> Option<DAUx::plugin_bridge::SharedPluginBridgeSink> {
        let region = self.shared_audio.get(instance_id)?;
        Some(sphere_plugin_host::plugin_bridge_sink::SharedRegionSink::into_shared(
            region.clone(),
        ))
    }

    pub fn loaded_descriptor(&self, instance: &str) -> Option<BridgeLoadedPlugin> {
        self.loaded.get(instance).cloned()
    }

    pub fn loaded_instance_ids(&self) -> Vec<String> {
        self.loaded.keys().cloned().collect()
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
        Ok(())
    }

    /// Stage 2: create a named shared-memory region for one insert and map it in
    /// the host. Idempotent per `instance_id`.
    fn establish_shared_audio_for_instance(
        &mut self,
        instance_id: &str,
        sample_rate: u32,
        max_block_size: u32,
    ) {
        if self.shared_audio.contains_key(instance_id) {
            return;
        }
        #[cfg(windows)]
        {
            use sphere_plugin_host::audio_bridge::SharedAudioRegion;
            let name = bridge_region_name(instance_id);
            match SharedAudioRegion::create_named(&name, sample_rate, max_block_size, 2) {
                Ok(region) => {
                    let bytes = region.bytes();
                    eprintln!(
                        "[plugin-bridge] shared audio region created instance={instance_id} name={name} bytes={bytes} sr={sample_rate} block={max_block_size}"
                    );
                    eprintln!(
                        "[plugin-bridge] sending AttachSharedAudio instance={instance_id} name={name} bytes={bytes}"
                    );
                    match self.client.attach_shared_audio(
                        name.clone(),
                        bytes,
                        instance_id.to_string(),
                    ) {
                        Ok(()) => {
                            self.shared_audio
                                .insert(instance_id.to_string(), Arc::new(region));
                        }
                        Err(error) => {
                            eprintln!(
                                "[plugin-bridge] AttachSharedAudio send failed instance={instance_id}: {error}"
                            )
                        }
                    }
                }
                Err(error) => {
                    eprintln!(
                        "[plugin-bridge] shared audio region create failed instance={instance_id}: {error}"
                    )
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (instance_id, sample_rate, max_block_size);
        }
    }

    pub fn send_load_plugin(
        &mut self,
        descriptor: BridgePluginDescriptor,
        sample_rate: u32,
        max_block_size: u32,
    ) -> Result<(), PluginHostClientError> {
        let _ = self.configure_audio_bridge(sample_rate, max_block_size);
        let instance = descriptor.insert_id.clone();
        eprintln!(
            "[plugin-bridge] sending LoadPlugin instance={} path={}",
            instance, descriptor.plugin_path
        );
        self.establish_shared_audio_for_instance(&instance, sample_rate, max_block_size);
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
        let input_channels = 2u32;
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
        self.shared_audio.remove(&plugin_instance_id);
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
            bridge.shared_audio.clear();
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
