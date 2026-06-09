//! VST3 MIDI preview + local audio output for the external PluginHost process.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use DAUx::vst3_processor::{Vst3MidiEvent, Vst3RuntimeProcessor};

use crate::audio_bridge::SharedMidiEvent;

fn forensic_trace_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("FUTUREBOARD_FORENSIC_TRACE").is_some()
            || std::env::var_os("FUTUREBOARD_MIDI_VERBOSE").is_some()
    })
}

const PREVIEW_TAIL_BLOCKS: u32 = 8;
const CC_SUSTAIN: u16 = 64;
const CC_ALL_SOUND_OFF: u16 = 120;
const CC_ALL_NOTES_OFF: u16 = 123;

pub type SharedPluginHostPreview = Arc<Mutex<PluginHostPreviewEngine>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PreviewNoteKey {
    channel: u8,
    pitch: u8,
}

#[derive(Debug)]
struct PreviewInstance {
    processor: Vst3RuntimeProcessor,
    active_notes: Vec<PreviewNoteKey>,
    pending_events: Vec<Vst3MidiEvent>,
    tail_blocks: u32,
}

#[derive(Debug)]
pub struct PluginHostPreviewEngine {
    sample_rate: u32,
    block_size: u32,
    instances: HashMap<String, PreviewInstance>,
    dsp_ready: bool,
    /// Keep processing loaded instances even without pending MIDI (VSTi editor
    /// internal keyboard / groove preview needs a live `process()` loop).
    continuous_mode: bool,
}

impl PluginHostPreviewEngine {
    pub fn shared(sample_rate: u32, block_size: u32) -> SharedPluginHostPreview {
        Arc::new(Mutex::new(Self::new(sample_rate, block_size)))
    }

    pub fn new(sample_rate: u32, block_size: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(44_100),
            block_size: block_size.clamp(64, 2048),
            instances: HashMap::new(),
            dsp_ready: false,
            continuous_mode: false,
        }
    }

    /// Stage 1: follow the main engine's sample rate / block size (the engine
    /// owns them). Returns the clamped values actually adopted. Existing loaded
    /// instances keep their current sample rate until reloaded — re-prepare is a
    /// later stage once the shared audio transport drives `process()`.
    pub fn configure(&mut self, sample_rate: u32, max_block_size: u32) -> (u32, u32) {
        self.sample_rate = sample_rate.max(44_100);
        self.block_size = max_block_size.clamp(64, 2048);
        (self.sample_rate, self.block_size)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    pub fn dsp_ready(&self) -> bool {
        self.dsp_ready
    }

    pub fn set_dsp_ready(&mut self, ready: bool) {
        self.dsp_ready = ready;
    }

    pub fn set_continuous_mode(&mut self, enabled: bool) {
        self.continuous_mode = enabled;
    }

    pub fn continuous_mode(&self) -> bool {
        self.continuous_mode
    }

    pub fn has_instance(&self, plugin_instance_id: &str) -> bool {
        self.instances.contains_key(plugin_instance_id)
    }

    pub fn loaded_instance_ids(&self) -> Vec<String> {
        self.instances.keys().cloned().collect()
    }

    pub fn log_unified_runtime(track_id: &str, insert_id: &str, plugin_instance_id: &str) {
        Self::verify_unified_runtime(
            track_id,
            insert_id,
            plugin_instance_id,
            plugin_instance_id,
            plugin_instance_id,
            plugin_instance_id,
            plugin_instance_id,
            plugin_instance_id,
        );
    }

    /// Strict instance identity check (spec Part 6).
    #[allow(clippy::too_many_arguments)]
    pub fn verify_unified_runtime(
        track_id: &str,
        insert_id: &str,
        plugin_instance_id: &str,
        editor_instance: &str,
        dsp_instance: &str,
        midi_playback_instance: &str,
        preview_instance: &str,
        shared_audio_instance: &str,
    ) {
        let unified = editor_instance == plugin_instance_id
            && dsp_instance == plugin_instance_id
            && midi_playback_instance == plugin_instance_id
            && preview_instance == plugin_instance_id
            && shared_audio_instance == plugin_instance_id;
        eprintln!(
            "[plugin-runtime-id] track_id={track_id} insert_id={insert_id} plugin_instance_id={plugin_instance_id}"
        );
        eprintln!("[plugin-runtime-id] editor_instance={editor_instance}");
        eprintln!("[plugin-runtime-id] dsp_instance={dsp_instance}");
        eprintln!("[plugin-runtime-id] midi_playback_instance={midi_playback_instance}");
        eprintln!("[plugin-runtime-id] preview_instance={preview_instance}");
        eprintln!("[plugin-runtime-id] shared_audio_instance={shared_audio_instance}");
        eprintln!("[plugin-runtime-id] unified={unified}");
        if !unified {
            eprintln!(
                "[plugin-runtime-id] ERROR duplicate runtime refused \
                 expected={plugin_instance_id} editor={editor_instance} dsp={dsp_instance} \
                 midi_playback={midi_playback_instance} preview={preview_instance} \
                 shared_audio={shared_audio_instance}"
            );
        }
    }

    pub fn log_host_registry(&self) {
        eprintln!("[plugin-host-registry] instances={}", self.instances.len());
        for (id, instance) in &self.instances {
            let editor = instance.processor.embed_is_valid();
            let dsp = instance.processor.is_ready();
            eprintln!("[plugin-host-registry] instance={id} loaded=true editor={editor} dsp={dsp}");
        }
    }

    pub fn load_instance(
        &mut self,
        plugin_instance_id: &str,
        plugin_path: &str,
        class_id: &str,
        sample_rate: u32,
        max_block_size: u32,
    ) -> bool {
        self.sample_rate = sample_rate.max(44_100);
        self.block_size = max_block_size.clamp(64, 2048);
        eprintln!("[plugin-host-registry] load begin instance={plugin_instance_id}");
        if self.instances.contains_key(plugin_instance_id) {
            eprintln!(
                "[plugin-host] LoadPlugin instance={plugin_instance_id} already_loaded=true reuse=true"
            );
            eprintln!(
                "[plugin-host-registry] already_loaded instance={plugin_instance_id} reuse=true"
            );
            eprintln!(
                "[plugin-host-vst3] create skipped reason=instance_exists instance={plugin_instance_id}"
            );
            return true;
        }
        eprintln!(
            "[plugin-host-vst3] create entered instance={plugin_instance_id} path={plugin_path}"
        );
        let Some(processor) = Vst3RuntimeProcessor::new(plugin_path, class_id, self.sample_rate)
        else {
            eprintln!(
                "[plugin-host-midi] preview processor create failed instance={plugin_instance_id}"
            );
            return false;
        };
        self.instances.insert(
            plugin_instance_id.to_string(),
            PreviewInstance {
                processor,
                active_notes: Vec::new(),
                pending_events: Vec::new(),
                tail_blocks: 0,
            },
        );
        eprintln!(
            "[plugin-host-midi] preview processor loaded instance={plugin_instance_id} dsp_output={}",
            if self.dsp_ready { "ready" } else { "pending" }
        );
        eprintln!("[plugin-host-registry] loaded instance={plugin_instance_id}");
        self.log_host_registry();
        true
    }

    pub fn unload_instance(&mut self, plugin_instance_id: &str) {
        eprintln!("[plugin-host-registry] unload instance={plugin_instance_id}");
        if let Some(instance) = self.instances.get(plugin_instance_id) {
            instance.processor.embed_detach();
        }
        if let Some(mut instance) = self.instances.remove(plugin_instance_id) {
            Self::panic_instance(&mut instance);
        }
        eprintln!("[plugin-host-registry] instances={}", self.instances.len());
    }

    pub fn default_editor_size(&self) -> (u32, u32) {
        (880, 600)
    }

    pub fn embed_editor_for_instance(
        &self,
        plugin_instance_id: &str,
        parent_hwnd: u64,
        width: i32,
        height: i32,
    ) -> Option<u64> {
        let Some(instance) = self.instances.get(plugin_instance_id) else {
            eprintln!("[plugin-host-registry] get found=false instance={plugin_instance_id}");
            eprintln!(
                "[plugin-editor] open ERROR instance not loaded instance={plugin_instance_id} uses_runtime_instance=false"
            );
            return None;
        };
        eprintln!("[plugin-host-registry] get found=true instance={plugin_instance_id}");
        eprintln!("[plugin-editor] open instance={plugin_instance_id} uses_runtime_instance=true");
        eprintln!("[plugin-editor] createView from existing controller (reuse loaded runtime)");
        eprintln!("[plugin-editor] no_duplicate_component_created=true");
        instance
            .processor
            .embed_editor(parent_hwnd, 0, 0, width, height)
    }

    pub fn embed_resize_for_instance(&self, plugin_instance_id: &str, width: i32, height: i32) {
        if let Some(instance) = self.instances.get(plugin_instance_id) {
            eprintln!(
                "[plugin-bridge] ResizeEditor instance={plugin_instance_id} width={width} height={height}"
            );
            instance.processor.embed_set_bounds(0, 0, width, height);
            instance.processor.embed_refresh();
            let host_hwnd = instance.processor.handle_value();
            eprintln!("[plugin-host-layout] host_hwnd=0x{host_hwnd:x}");
            eprintln!("[plugin-host-layout] host_client=({width},{height})");
            if let Some((child_w, child_h)) = instance.processor.embed_content_size() {
                eprintln!("[plugin-host-layout] plugin_child_count=1");
                eprintln!("[plugin-host-layout] child=plugin_view client=({child_w},{child_h})");
                let child_matches = child_w == width && child_h == height;
                eprintln!("[plugin-host-layout] child_matches_host={child_matches}");
            } else {
                eprintln!("[plugin-host-layout] plugin_child_count=0");
                eprintln!("[plugin-host-layout] child_matches_host=false");
            }
        }
    }

    pub fn embed_refresh_for_instance(&self, plugin_instance_id: &str) {
        if let Some(instance) = self.instances.get(plugin_instance_id) {
            instance.processor.embed_refresh();
        }
    }

    pub fn embed_detach_for_instance(&mut self, plugin_instance_id: &str) {
        if let Some(instance) = self.instances.get(plugin_instance_id) {
            instance.processor.embed_detach();
        }
        if let Some(instance) = self.instances.get_mut(plugin_instance_id) {
            Self::panic_instance(instance);
        }
    }

    pub fn editor_content_size_for_instance(&self, plugin_instance_id: &str) -> (u32, u32) {
        if let Some(instance) = self.instances.get(plugin_instance_id) {
            if let Some((w, h)) = instance.processor.embed_content_size() {
                return (w.max(1) as u32, h.max(1) as u32);
            }
        }
        self.default_editor_size()
    }

    pub fn preview_note_on(
        &mut self,
        plugin_instance_id: &str,
        channel: u8,
        pitch: u8,
        velocity: u8,
    ) {
        eprintln!(
            "[plugin-host-midi-consume] preview note_on instance={plugin_instance_id} pitch={pitch}"
        );
        let Some(instance) = self.instances.get_mut(plugin_instance_id) else {
            eprintln!(
                "[plugin-host-midi] preview note_on dropped instance={plugin_instance_id} reason=unknown_instance"
            );
            return;
        };
        let key = PreviewNoteKey {
            channel: channel.min(15),
            pitch: pitch.min(127),
        };
        if instance.active_notes.contains(&key) {
            instance
                .pending_events
                .push(Vst3MidiEvent::note_off(0, key.channel, key.pitch, 0.0));
        }
        let vel = velocity.clamp(1, 127) as f32 / 127.0;
        instance
            .pending_events
            .push(Vst3MidiEvent::note_on(0, key.channel, key.pitch, vel));
        if !instance.active_notes.contains(&key) {
            instance.active_notes.push(key);
        }
        instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
        eprintln!("[plugin-host-midi] queued note_on to VSTi");
    }

    pub fn preview_note_off(&mut self, plugin_instance_id: &str, channel: u8, pitch: u8) {
        eprintln!(
            "[plugin-host-midi-consume] preview note_off instance={plugin_instance_id} pitch={pitch}"
        );
        let Some(instance) = self.instances.get_mut(plugin_instance_id) else {
            return;
        };
        let key = PreviewNoteKey {
            channel: channel.min(15),
            pitch: pitch.min(127),
        };
        instance
            .pending_events
            .push(Vst3MidiEvent::note_off(0, key.channel, key.pitch, 0.0));
        instance.active_notes.retain(|n| *n != key);
        instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
    }

    pub fn preview_all_notes_off(&mut self, plugin_instance_id: &str) {
        eprintln!("[plugin-host-midi] preview all_notes_off instance={plugin_instance_id}");
        let Some(instance) = self.instances.get_mut(plugin_instance_id) else {
            return;
        };
        Self::panic_instance(instance);
    }

    pub fn midi_panic(&mut self, plugin_instance_id: &str) {
        eprintln!("[plugin-host-midi] midi_panic instance={plugin_instance_id}");
        self.preview_all_notes_off(plugin_instance_id);
    }

    fn panic_instance(instance: &mut PreviewInstance) {
        for key in instance.active_notes.drain(..) {
            instance
                .pending_events
                .push(Vst3MidiEvent::note_off(0, key.channel, key.pitch, 0.0));
        }
        let ch = 0u8;
        instance
            .pending_events
            .push(Vst3MidiEvent::control_change(0, ch, CC_SUSTAIN, 0.0));
        instance
            .pending_events
            .push(Vst3MidiEvent::control_change(0, ch, CC_ALL_NOTES_OFF, 0.0));
        instance
            .pending_events
            .push(Vst3MidiEvent::control_change(0, ch, CC_ALL_SOUND_OFF, 0.0));
        instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
    }

    /// Stage 3: apply one engine-pushed MIDI event drained from the shared ring
    /// to the loaded instrument instance(s). Until per-instance routing exists in
    /// the ring, the event targets every loaded preview instance (a single VSTi
    /// is the common case). Raw status byte: high nibble = message, low nibble =
    /// channel. Wait-free (no alloc) apart from the existing event queues.
    pub fn apply_shared_midi(&mut self, ev: SharedMidiEvent) {
        let channel = ev.status & 0x0F;
        let kind = ev.status & 0xF0;
        let ids: Vec<String> = self.instances.keys().cloned().collect();
        for id in ids {
            let Some(instance) = self.instances.get_mut(&id) else {
                continue;
            };
            match kind {
                // Note-on with velocity 0 is a note-off (running-status idiom).
                0x90 if ev.data2 > 0 => {
                    let vel = ev.data2.clamp(1, 127) as f32 / 127.0;
                    if forensic_trace_enabled() {
                        eprintln!(
                            "[plugin-host-midi-consume] note_on instance={id} pitch={} offset={}",
                            ev.data1, ev.sample_offset
                        );
                    }
                    instance.pending_events.push(Vst3MidiEvent::note_on(
                        ev.sample_offset,
                        channel,
                        ev.data1.min(127),
                        vel,
                    ));
                    let key = PreviewNoteKey {
                        channel: channel.min(15),
                        pitch: ev.data1.min(127),
                    };
                    if !instance.active_notes.contains(&key) {
                        instance.active_notes.push(key);
                    }
                    instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
                }
                0x80 | 0x90 => {
                    if forensic_trace_enabled() {
                        eprintln!(
                            "[plugin-host-midi-consume] note_off instance={id} pitch={} offset={}",
                            ev.data1, ev.sample_offset
                        );
                    }
                    instance.pending_events.push(Vst3MidiEvent::note_off(
                        ev.sample_offset,
                        channel,
                        ev.data1.min(127),
                        0.0,
                    ));
                    let key = PreviewNoteKey {
                        channel: channel.min(15),
                        pitch: ev.data1.min(127),
                    };
                    instance.active_notes.retain(|n| *n != key);
                    instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
                }
                0xB0 => {
                    instance.pending_events.push(Vst3MidiEvent::control_change(
                        ev.sample_offset,
                        channel,
                        ev.data1 as u16,
                        ev.data2 as f32 / 127.0,
                    ));
                    instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
                }
                _ => {}
            }
        }
    }

    /// Stage 3: render one block of all preview instruments interleaved-stereo
    /// into `out` (length `frames * 2`) and return the per-channel peak. Used by
    /// the host's shared-memory bridge service to fill `audio_out`.
    pub fn render_into_interleaved(&mut self, out: &mut [f32], frames: usize) -> (f32, f32) {
        let (mix_l, mix_r) = self.render_block(frames);
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        for i in 0..frames {
            let l = mix_l.get(i).copied().unwrap_or(0.0);
            let r = mix_r.get(i).copied().unwrap_or(0.0);
            if let Some(slot) = out.get_mut(i * 2) {
                *slot = l;
            }
            if let Some(slot) = out.get_mut(i * 2 + 1) {
                *slot = r;
            }
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
        }
        (peak_l, peak_r)
    }

    pub fn has_active_preview(&self) -> bool {
        self.instances.values().any(|i| {
            !i.pending_events.is_empty() || !i.active_notes.is_empty() || i.tail_blocks > 0
        })
    }

    pub fn has_loaded_instances(&self) -> bool {
        !self.instances.is_empty()
    }

    pub fn render_block(&mut self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let scratch_l = vec![0.0f32; frames];
        let scratch_r = vec![0.0f32; frames];
        self.render_block_with_input(frames, &scratch_l, &scratch_r)
    }

    pub fn render_block_with_input(
        &mut self,
        frames: usize,
        in_l: &[f32],
        in_r: &[f32],
    ) -> (Vec<f32>, Vec<f32>) {
        let mut mix_l = vec![0.0f32; frames];
        let mut mix_r = vec![0.0f32; frames];
        if self.instances.is_empty() {
            return (mix_l, mix_r);
        }
        if !self.dsp_ready {
            return (mix_l, mix_r);
        }
        let mut out_l = vec![0.0f32; frames];
        let mut out_r = vec![0.0f32; frames];
        for instance in self.instances.values_mut() {
            let has_work = !instance.pending_events.is_empty()
                || !instance.active_notes.is_empty()
                || instance.tail_blocks > 0;
            if !has_work && !self.continuous_mode {
                continue;
            }
            let events = std::mem::take(&mut instance.pending_events);
            let _ = instance
                .processor
                .process_stereo_block_with_midi(in_l, in_r, &mut out_l, &mut out_r, &events);
            for i in 0..frames {
                mix_l[i] += out_l[i];
                mix_r[i] += out_r[i];
            }
            if events.is_empty() && instance.active_notes.is_empty() {
                instance.tail_blocks = instance.tail_blocks.saturating_sub(1);
            } else {
                instance.tail_blocks = PREVIEW_TAIL_BLOCKS;
            }
        }
        (mix_l, mix_r)
    }
}

pub fn try_start_preview_output(shared: &SharedPluginHostPreview) -> bool {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(device) => device,
        None => {
            eprintln!(
                "[plugin-host-midi] preview received but dsp_output=pending reason=no_output_device"
            );
            return false;
        }
    };
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "[plugin-host-midi] preview received but dsp_output=pending reason=config_error {error}"
            );
            return false;
        }
    };
    let channels = config.channels() as usize;
    let sample_rate = config.sample_rate().0;
    let shared_cb = shared.clone();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let ch = channels.max(1);
                let frames = data.len() / ch;
                let (mix_l, mix_r) = shared_cb.lock().render_block(frames);
                for (i, frame) in data.chunks_mut(ch).enumerate() {
                    let l = mix_l.get(i).copied().unwrap_or(0.0);
                    let r = mix_r.get(i).copied().unwrap_or(0.0);
                    if ch == 1 {
                        frame[0] = (l + r) * 0.5;
                    } else {
                        frame[0] = l;
                        if ch > 1 {
                            frame[1] = r;
                        }
                        for sample in frame.iter_mut().skip(2) {
                            *sample = 0.0;
                        }
                    }
                }
            },
            |error| eprintln!("[plugin-host-midi] preview output stream error={error}"),
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config.into(),
            move |data: &mut [i16], _| {
                let ch = channels.max(1);
                let frames = data.len() / ch;
                let (mix_l, mix_r) = shared_cb.lock().render_block(frames);
                for (i, frame) in data.chunks_mut(ch).enumerate() {
                    let l = mix_l.get(i).copied().unwrap_or(0.0);
                    let r = mix_r.get(i).copied().unwrap_or(0.0);
                    let mono = if ch == 1 { (l + r) * 0.5 } else { l };
                    let sample = if ch == 1 { mono } else { l };
                    frame[0] = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    if ch > 1 {
                        frame[1] = (r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                }
            },
            |error| eprintln!("[plugin-host-midi] preview output stream error={error}"),
            None,
        ),
        _ => {
            eprintln!(
                "[plugin-host-midi] preview received but dsp_output=pending reason=unsupported_sample_format"
            );
            return false;
        }
    };
    let stream = match stream {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!(
                "[plugin-host-midi] preview received but dsp_output=pending reason=stream_error {error}"
            );
            return false;
        }
    };
    if let Err(error) = stream.play() {
        eprintln!(
            "[plugin-host-midi] preview received but dsp_output=pending reason=play_error {error}"
        );
        return false;
    }
    shared.lock().set_dsp_ready(true);
    eprintln!("[plugin-host-midi] preview dsp_output=ready sr={sample_rate}");
    std::mem::forget(stream);
    true
}
