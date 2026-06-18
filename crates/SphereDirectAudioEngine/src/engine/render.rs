//! DSP / render kernel for the native audio engine.
//!
//! Split out of `engine.rs` (which owns device lifecycle, command dispatch, and
//! the public engine API) so the realtime render path can be read and modified
//! in isolation. This is a pure relocation of the free render/DSP functions that
//! previously lived inline in `engine.rs` — no behavior change.
//!
//! Realtime rules apply to everything reachable from
//! `render_project_block_interleaved`: no allocation, no locking, no blocking in
//! steady state. `use super::*;` pulls in the shared engine vocabulary
//! (`SharedState`, runtime types, consts, debug-flag helpers).
use super::*;
use SphereAudioProcessor::StretchBackend;

#[inline]
pub fn render_project_sample(
    runtime: &mut RuntimeProject,
    project_sample: u64,
    master_volume: f32,
) -> (f32, f32) {
    let mut out_l = 0.0f32;
    let mut out_r = 0.0f32;
    let master_index = runtime.tracks.iter().position(|t| t.track_type == "master");
    let beat = sample_to_beat(runtime, project_sample);

    for clip_index in 0..runtime.clips.len() {
        let clip = &runtime.clips[clip_index];
        if clip.muted {
            continue;
        }
        let clip_start_sample = clip.start_sample;
        let clip_duration_samples = clip.duration_samples;
        if project_sample < clip_start_sample {
            continue;
        }
        let rel = project_sample - clip_start_sample;
        if rel >= clip_duration_samples {
            continue;
        }

        let clip_offset_seconds = clip.offset_seconds;
        let clip_source_read_rate = clip.source_read_rate;
        let clip_reverse = clip.reverse;
        let clip_gain = clip.gain;
        let clip_fade_in = clip.fade_in_samples;
        let clip_fade_out = clip.fade_out_samples;
        let source = Arc::clone(&clip.source);

        // Resolved at build time — no id lookup or String clone per sample.
        let Some(track_index) = clip.track_index.filter(|&ti| ti < runtime.tracks.len()) else {
            continue;
        };
        if Some(track_index) == master_index {
            continue;
        }
        let has_solo = runtime.has_solo;
        if effective_track_muted(&runtime.tracks[track_index], beat)
            || (has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }

        let source_pos_seconds = clip_source_pos_seconds(
            clip_offset_seconds,
            rel,
            clip_duration_samples,
            runtime.sample_rate,
            if matches!(clip.processor, ClipDspProcessor::PhaseVocoderBasic) {
                1.0 / clip.effective_time_ratio.max(0.01)
            } else {
                clip_source_read_rate
            },
            clip_reverse,
        );
        let source_pos = source_pos_seconds * source.sample_rate() as f64;
        let dry_pos_seconds = clip_source_pos_seconds(
            clip_offset_seconds,
            rel,
            clip_duration_samples,
            runtime.sample_rate,
            clip_source_read_rate,
            clip_reverse,
        );
        let dry_source_pos = dry_pos_seconds * source.sample_rate() as f64;
        let (mut l, mut r) = sample_clip_processor_stereo(
            &source,
            source_pos,
            dry_source_pos,
            clip.effective_time_ratio,
            clip.processor,
        );
        if l == 0.0 && r == 0.0 {
            continue;
        }

        let fade = clip_fade_gain(rel, clip_duration_samples, clip_fade_in, clip_fade_out);
        let g = clip_gain * fade;
        l *= g;
        r *= g;

        // Build-time resolved output index (None for master/missing) — never
        // clone ids or the sends Vec on the audio thread.
        let output_track_index = runtime.tracks[track_index]
            .output_track_index
            .filter(|&t| t < runtime.tracks.len());
        let (track_l, track_r) =
            apply_track_chain_at_beat(l, r, &mut runtime.tracks[track_index], beat);
        let (track_l, track_r) =
            apply_preview_mode(track_l, track_r, runtime.tracks[track_index].preview_mode);
        runtime.accumulate_track_meter(track_index, track_l, track_r);

        if let Some(target_index) = output_track_index {
            let (bus_l, bus_r) = apply_track_chain_at_beat(
                track_l,
                track_r,
                &mut runtime.tracks[target_index],
                beat,
            );
            let (bus_l, bus_r) =
                apply_preview_mode(bus_l, bus_r, runtime.tracks[target_index].preview_mode);
            runtime.accumulate_track_meter(target_index, bus_l, bus_r);
            out_l += bus_l;
            out_r += bus_r;
        } else {
            out_l += track_l;
            out_r += track_r;
        }

        let send_count = runtime.tracks[track_index].sends.len();
        for s in 0..send_count {
            let (enabled, level, return_track_index) = {
                let send = &runtime.tracks[track_index].sends[s];
                (send.enabled, send.level, send.return_track_index)
            };
            if !enabled || level <= 0.0 {
                continue;
            }
            let Some(return_track_index) = return_track_index.filter(|&t| t < runtime.tracks.len())
            else {
                continue;
            };
            let return_track = &runtime.tracks[return_track_index];
            if effective_track_muted(return_track, beat) || (runtime.has_solo && !return_track.solo)
            {
                continue;
            }
            let (send_l, send_r) = apply_track_chain_at_beat(
                track_l * level,
                track_r * level,
                &mut runtime.tracks[return_track_index],
                beat,
            );
            let (send_l, send_r) = apply_preview_mode(
                send_l,
                send_r,
                runtime.tracks[return_track_index].preview_mode,
            );
            runtime.accumulate_track_meter(return_track_index, send_l, send_r);
            out_l += send_l;
            out_r += send_r;
        }
    }

    // ── Master bus: apply master track inserts on the summed output ──
    if let Some(m_idx) = master_index {
        let muted = effective_track_muted(&runtime.tracks[m_idx], beat)
            || (runtime.has_solo && !runtime.tracks[m_idx].solo);
        if !muted {
            let master = &mut runtime.tracks[m_idx];
            for insert in &mut master.inserts {
                let (l, r) = apply_insert(out_l, out_r, insert);
                out_l = l;
                out_r = r;
            }
            let (l, r) = apply_preview_mode(out_l, out_r, master.preview_mode);
            out_l = l;
            out_r = r;
            runtime.accumulate_track_meter(m_idx, out_l, out_r);
        }
    }

    (
        crate::dsp::gain::soft_limit(out_l * master_volume),
        crate::dsp::gain::soft_limit(out_r * master_volume),
    )
}

/// Routing track kinds (Phase 3): receive sends rather than hosting clips.
#[inline]
fn is_routing_type(track_type: &str) -> bool {
    is_routing_track_type(track_type)
}

/// Two distinct mutable elements of a slice without allocation. Panics in
/// debug if `a == b`; callers guarantee distinct indices.
#[inline]
fn two_mut<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    debug_assert!(a != b);
    if a < b {
        let (lo, hi) = v.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = v.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}

#[inline]
pub(crate) fn tempo_map_from_project_snapshot(project: &EngineProjectSnapshot) -> TempoMap {
    if project.tempo_points.is_empty() {
        TempoMap::static_tempo(project.bpm)
    } else {
        TempoMap::from_points(
            project.bpm,
            project
                .tempo_points
                .iter()
                .map(|p| TempoPoint {
                    beat: p.beat,
                    bpm: p.bpm,
                })
                .collect(),
        )
    }
}

fn sample_to_beat(runtime: &RuntimeProject, sample: u64) -> f64 {
    runtime
        .tempo_map
        .beat_at_samples(sample, runtime.sample_rate.max(1) as f64)
}

/// Map an in-clip output offset `rel` to a source position in **seconds**,
/// honoring the clip's resample `speed_ratio` and reverse flag.
///
/// Forward playback reads from `offset_seconds` and advances at `speed_ratio`
/// source-seconds per output-second. Reverse reads the same source window from
/// its end backward, so output sample 0 maps to the last source frame and the
/// final output sample maps back to `offset_seconds`. Allocation-free; called
/// from the audio callback.
#[inline]
pub(crate) fn clip_source_pos_seconds(
    offset_seconds: f64,
    rel: u64,
    duration_samples: u64,
    output_sample_rate: u32,
    speed_ratio: f32,
    reverse: bool,
) -> f64 {
    let sr = output_sample_rate.max(1) as f64;
    let advance = if reverse {
        duration_samples.saturating_sub(1).saturating_sub(rel)
    } else {
        rel
    } as f64;
    offset_seconds + (advance / sr) * speed_ratio as f64
}

#[inline]
pub(crate) fn sample_clip_processor_stereo(
    source: &ClipAudioSource,
    source_pos: f64,
    resample_pos: f64,
    effective_time_ratio: f32,
    processor: ClipDspProcessor,
) -> (f32, f32) {
    if !matches!(processor, ClipDspProcessor::PhaseVocoderBasic) {
        return sample_source_stereo(source, resample_pos);
    }
    phase_vocoder_basic_sample(source, source_pos, effective_time_ratio)
}

#[inline]
fn phase_vocoder_basic_sample(
    source: &ClipAudioSource,
    source_pos: f64,
    effective_time_ratio: f32,
) -> (f32, f32) {
    let ratio = effective_time_ratio.clamp(0.05, 20.0) as f64;
    if (ratio - 1.0).abs() < 1e-6 {
        return sample_source_stereo(source, source_pos);
    }

    // Basic streaming OLA/granular stretcher. It is allocation-free and reads
    // from the existing clip source; a higher-quality phase vocoder can replace
    // this processor without changing snapshot/runtime routing.
    let grain = 1024.0_f64;
    let hop_out = grain * 0.5;
    let hop_in = hop_out / ratio;
    let phase = (source_pos / hop_in).fract().clamp(0.0, 1.0);
    let window = 0.5 - 0.5 * (std::f64::consts::TAU * phase).cos();
    let (al, ar) = sample_source_stereo(source, source_pos);
    let (bl, br) = sample_source_stereo(source, source_pos + hop_in);
    let w = window as f32;
    (al * (1.0 - w) + bl * w, ar * (1.0 - w) + br * w)
}

/// Linear clip-fade gain for a sample at offset `rel` from the clip start.
///
/// `1.0` outside both fade regions; ramps `0→1` across the fade-in and `1→0`
/// across the fade-out. Linear is the current placeholder shape — the snapshot
/// carries per-fade curve names (`audio-system-plan.md` §6) which a later slice
/// can map to equal-power / exponential shaping here. Allocation-free.
#[inline]
pub(crate) fn clip_fade_gain(rel: u64, duration: u64, fade_in: u64, fade_out: u64) -> f32 {
    let mut gain = 1.0f32;
    if fade_in > 0 && rel < fade_in {
        gain *= rel as f32 / fade_in as f32;
    }
    if fade_out > 0 {
        let fade_out_start = duration.saturating_sub(fade_out);
        if rel >= fade_out_start {
            let into = (rel - fade_out_start) as f32;
            gain *= (1.0 - into / fade_out as f32).max(0.0);
        }
    }
    gain
}

#[inline]
pub(crate) fn effective_track_muted(track: &RuntimeTrack, beat: f64) -> bool {
    track
        .automation_values_at_beat(beat)
        .muted
        .unwrap_or(track.muted)
}

/// Apply a track's fader (volume / pan / preview mode) to its `block_*`
/// (which already holds the post-insert signal), write the post-fader result
/// back into `block_*`, and accumulate the track meter. Does **not** sum to any
/// destination — routing is done separately by [`route_main_output`]. No
/// allocation.
#[inline]
fn apply_fader(track: &mut RuntimeTrack, frames: usize, beat: f64) {
    let automation = track.automation_values_at_beat(beat);
    let volume = automation.volume.unwrap_or(track.volume);
    let pan = automation.pan.unwrap_or(track.pan);
    let (pan_l, pan_r) = pan_gains(pan);
    for frame_idx in 0..frames {
        let (l, r) = apply_preview_mode(
            track.block_l[frame_idx] * volume * pan_l,
            track.block_r[frame_idx] * volume * pan_r,
            track.preview_mode,
        );
        track.block_l[frame_idx] = l;
        track.block_r[frame_idx] = r;
    }
}

#[inline]
fn accumulate_block_meter(track: &mut RuntimeTrack, frames: usize) {
    for frame_idx in 0..frames {
        let l = track.block_l[frame_idx];
        let r = track.block_r[frame_idx];
        track.meter_peak_l = track.meter_peak_l.max(l.abs());
        track.meter_peak_r = track.meter_peak_r.max(r.abs());
        track.meter_sum_sq_l += l * l;
        track.meter_sum_sq_r += r * r;
    }
}

/// Sum a track's post-fader `block_*` into its output destination.
///
/// If `output_track_id` resolves to a routing track (bus/group/return) the
/// full post-fader signal is added to that track's receive buffer (`recv_*`),
/// so it is processed in Pass 2; otherwise it sums into the interleaved master
/// output. Cycle-safe like [`accumulate_sends`]: routing to self, to a
/// non-routing track, or backward between routing tracks falls back to master.
/// No allocation.
#[inline]
pub(crate) fn route_main_output(
    runtime: &mut RuntimeProject,
    src_index: usize,
    frames: usize,
    output: &mut [f32],
    channels: usize,
) {
    // Resolved at build time (None for master/missing) — no id lookup on the
    // audio thread.
    let target = runtime.tracks[src_index]
        .output_track_index
        .filter(|&t| t < runtime.tracks.len());

    if let Some(t) = target {
        let src_routing = is_routing_type(&runtime.tracks[src_index].track_type);
        let accept = t != src_index
            && is_routing_type(&runtime.tracks[t].track_type)
            && (!src_routing || t > src_index);
        if accept {
            let (src, tgt) = two_mut(&mut runtime.tracks, src_index, t);
            for f in 0..frames {
                tgt.recv_l[f] += src.block_l[f];
                tgt.recv_r[f] += src.block_r[f];
            }
            return;
        }
    }

    // Default / fallback: sum into the master output.
    let track = &runtime.tracks[src_index];
    for f in 0..frames {
        let out = &mut output[f * channels..f * channels + channels];
        out[0] += track.block_l[f];
        out[1] += track.block_r[f];
    }
}

#[allow(clippy::too_many_arguments)]
fn process_track_block(
    runtime: &mut RuntimeProject,
    track_index: usize,
    frames: usize,
    output: &mut [f32],
    channels: usize,
    beat: f64,
    transport: RuntimeTransportContext,
) {
    apply_track_chain_block(&mut runtime.tracks[track_index], frames, true, transport);
    // Pre-fader sends tap the post-insert signal currently in block_*.
    accumulate_sends(runtime, track_index, frames, true);
    apply_fader(&mut runtime.tracks[track_index], frames, beat);
    let pdc_delay = runtime
        .latency_graph
        .track_pdc_delay
        .get(track_index)
        .copied()
        .unwrap_or(0);
    if pdc_delay > 0 {
        let track = &mut runtime.tracks[track_index];
        apply_pdc_delay_block(
            &mut track.block_l[..frames],
            &mut track.block_r[..frames],
            &mut track.pdc_delay_l,
            &mut track.pdc_delay_r,
            &mut track.pdc_write_pos,
            pdc_delay,
            frames,
        );
    }
    accumulate_block_meter(&mut runtime.tracks[track_index], frames);
    // Post-fader sends tap the post-fader (and PDC-aligned) signal in block_*.
    accumulate_sends(runtime, track_index, frames, false);
    // Route the post-fader signal to master or the track's output bus.
    route_main_output(runtime, track_index, frames, output, channels);
}

/// Add the source track's block (`block_*`, holding either the post-insert or
/// post-fader signal depending on `pre_fader`) into each accepted send target's
/// receive buffer (`recv_*`), scaled by the send level. Only sends whose
/// `pre_fader` flag matches the requested phase are routed.
///
/// Cycle-safe by construction: a send is accepted only when the target is a
/// routing track (bus/return); a *routing* source may additionally only target
/// a *later* routing track in array order. Sends to non-routing tracks, to
/// self, or backward between routing tracks are dropped (logged at build time
/// under `FUTUREBOARD_ROUTING_DEBUG`). No allocation on the audio thread.
#[inline]
pub(crate) fn accumulate_sends(
    runtime: &mut RuntimeProject,
    src_index: usize,
    frames: usize,
    pre_fader: bool,
) {
    let send_count = runtime.tracks[src_index].sends.len();
    if send_count == 0 {
        return;
    }
    let src_routing = is_routing_type(&runtime.tracks[src_index].track_type);
    for s in 0..send_count {
        let (enabled, level, target_index) = {
            let send = &runtime.tracks[src_index].sends[s];
            if send.pre_fader != pre_fader {
                continue;
            }
            (send.enabled, send.level, send.return_track_index)
        };
        if !enabled || level == 0.0 {
            continue;
        }
        // Resolved at build time — no id lookup on the audio thread.
        let Some(t) = target_index.filter(|&t| t < runtime.tracks.len()) else {
            continue;
        };
        if t == src_index || !is_routing_type(&runtime.tracks[t].track_type) {
            continue;
        }
        if src_routing && t <= src_index {
            continue;
        }
        let (src, tgt) = two_mut(&mut runtime.tracks, src_index, t);
        for f in 0..frames {
            tgt.recv_l[f] += src.block_l[f] * level;
            tgt.recv_r[f] += src.block_r[f] * level;
        }
    }
}

/// Source-stream span consumed to render the output segment
/// `[rel_start, rel_start + frames)` at `time_ratio`. Successive segments tile
/// the source contiguously — block N's `in_start + input_frames` equals block
/// N+1's `in_start` — so the source is read exactly once with no gap or overlap,
/// and the total consumed over the clip is `floor(duration / time_ratio)`
/// (= the source length), never more. This is what keeps the streaming stretcher
/// from over-reading the source or growing an internal backlog.
pub(crate) fn signalsmith_input_span(
    rel_start: u64,
    frames: usize,
    time_ratio: f64,
) -> (i64, usize) {
    let ratio = time_ratio.clamp(0.05, 20.0);
    let in_start = (rel_start as f64 / ratio).floor() as i64;
    let in_end = ((rel_start + frames as u64) as f64 / ratio).floor() as i64;
    (in_start, (in_end - in_start).max(1) as usize)
}

fn render_signalsmith_clip_segment(
    runtime: &mut RuntimeProject,
    clip_index: usize,
    track_index: usize,
    project_start_sample: u64,
    rel_start: u64,
    frame_idx_start: usize,
    frames: usize,
) -> bool {
    let (
        source,
        offset_seconds,
        duration_samples,
        output_sample_rate,
        reverse,
        gain,
        fade_in_samples,
        fade_out_samples,
        time_ratio,
    ) = {
        let clip = &runtime.clips[clip_index];
        (
            Arc::clone(&clip.source),
            clip.offset_seconds,
            clip.duration_samples,
            runtime.sample_rate,
            clip.reverse,
            clip.gain,
            clip.fade_in_samples,
            clip.fade_out_samples,
            clip.effective_time_ratio.clamp(0.05, 20.0) as f64,
        )
    };

    let clip = &mut runtime.clips[clip_index];
    let Some(processor) = clip.stretch_processor.as_mut() else {
        return false;
    };
    if clip.stretch_next_project_sample != Some(project_start_sample) {
        processor.reset();
    }

    // Map this output segment [rel_start, rel_start + frames) onto a *contiguous*
    // span of the source stream so successive blocks tile the source with no gap
    // or overlap, and the source is never over-read. The stretcher consumes
    // exactly these `input_frames` samples to produce `frames` output (time ratio
    // = frames / input_frames), so it never has to buffer/grow across calls.
    let (in_start, input_frames) = signalsmith_input_span(rel_start, frames, time_ratio);
    let total_input = (duration_samples as f64 / time_ratio).floor() as i64;
    let output_sr = output_sample_rate.max(1) as f64;
    let source_sr = source.sample_rate() as f64;

    if clip.stretch_input_l.len() < input_frames {
        clip.stretch_input_l.resize(input_frames, 0.0);
        clip.stretch_input_r.resize(input_frames, 0.0);
    }
    if clip.stretch_output_l.len() < frames {
        clip.stretch_output_l.resize(frames, 0.0);
        clip.stretch_output_r.resize(frames, 0.0);
    }

    for k in 0..input_frames {
        let stream_index = in_start + k as i64;
        let effective = if reverse {
            (total_input - 1 - stream_index).max(0)
        } else {
            stream_index
        };
        // Read the source at the output sample rate (seconds map handles the
        // source↔output rate conversion), matching the per-sample resample path.
        let source_pos = (offset_seconds + effective as f64 / output_sr) * source_sr;
        let (l, r) = sample_source_stereo(&source, source_pos);
        clip.stretch_input_l[k] = l;
        clip.stretch_input_r[k] = r;
    }

    if processor
        .process_stereo(
            &clip.stretch_input_l[..input_frames],
            &clip.stretch_input_r[..input_frames],
            &mut clip.stretch_output_l[..frames],
            &mut clip.stretch_output_r[..frames],
        )
        .is_err()
    {
        clip.stretch_next_project_sample = None;
        return false;
    }
    clip.stretch_next_project_sample = Some(project_start_sample + frames as u64);

    let track = &mut runtime.tracks[track_index];
    for i in 0..frames {
        let rel = rel_start + i as u64;
        let fade = clip_fade_gain(rel, duration_samples, fade_in_samples, fade_out_samples);
        let g = gain * fade;
        let frame_idx = frame_idx_start + i;
        track.block_l[frame_idx] += clip.stretch_output_l[i] * g;
        track.block_r[frame_idx] += clip.stretch_output_r[i] * g;
    }

    true
}

/// `transport_active` — false when this block is rendered while the transport
/// is stopped (MIDI preview, post-panic bridge flush, open plugin editor). In
/// that mode the track/insert graph still runs (so bridged VSTi previews are
/// heard and the host handshake stays alive) but timeline clip material is
/// skipped — otherwise the frozen playhead would stutter-loop the same audio
/// clip slice every callback.
#[allow(clippy::too_many_arguments)]
pub fn render_project_block_interleaved(
    runtime: &mut RuntimeProject,
    base_sample: u64,
    master_volume: f32,
    output: &mut [f32],
    channels: usize,
    transport_active: bool,
    time_sig_num: u32,
    time_sig_den: u32,
    loop_bounds: Option<crate::transport::LoopBounds>,
) -> u64 {
    if channels < 2 {
        return 0;
    }
    let frames = output.len() / channels;
    if frames == 0 {
        return 0;
    }
    runtime.refresh_runtime_latency_graph(frames as u32);
    let block_beat = sample_to_beat(runtime, base_sample);
    // Real transport ProcessContext for every plugin processed this block —
    // tempo from the map at this position, time signature from the engine,
    // project position from the playhead, playing = transport state. Replaces
    // the old hardcoded 120 BPM / always-playing stub.
    let transport = RuntimeTransportContext {
        tempo_bpm: runtime.tempo_map.bpm_at_beat(block_beat),
        time_sig_num,
        time_sig_den,
        project_time_samples: base_sample as i64,
        ppq_position: block_beat,
        bar_position_ppq: RuntimeTransportContext::bar_start_ppq(
            block_beat,
            time_sig_num,
            time_sig_den,
        ),
        playing: transport_active,
        recording: false,
    };
    for frame in output.chunks_mut(channels) {
        frame[0] = 0.0;
        frame[1] = 0.0;
        for extra in frame.iter_mut().skip(2) {
            *extra = 0.0;
        }
    }

    for track in &mut runtime.tracks {
        if track.block_l.len() < frames {
            track.block_l.resize(frames, 0.0);
            track.block_r.resize(frames, 0.0);
        }
        // Receive buffers grow lazily to the largest block seen; the audio
        // thread only `fill`s, never allocates, once warmed.
        if track.recv_l.len() < frames {
            track.recv_l.resize(frames, 0.0);
            track.recv_r.resize(frames, 0.0);
        }
        track.block_l[..frames].fill(0.0);
        track.block_r[..frames].fill(0.0);
        track.recv_l[..frames].fill(0.0);
        track.recv_r[..frames].fill(0.0);
    }

    let master_index = runtime.audio_graph.master_index;

    for clip_index in 0..runtime.clips.len() {
        if !transport_active {
            break; // stopped-transport preview block — no timeline material
        }
        let (
            clip_muted,
            clip_track_index,
            source,
            clip_start,
            clip_duration,
            clip_offset_seconds,
            clip_source_read_rate,
            clip_effective_time_ratio,
            clip_processor,
            clip_reverse,
            clip_gain,
            clip_fade_in,
            clip_fade_out,
            clip_stretch_backend,
        ) = {
            let clip = &runtime.clips[clip_index];
            (
                clip.muted,
                clip.track_index,
                Arc::clone(&clip.source),
                clip.start_sample,
                clip.duration_samples,
                clip.offset_seconds,
                clip.source_read_rate,
                clip.effective_time_ratio,
                clip.processor,
                clip.reverse,
                clip.gain,
                clip.fade_in_samples,
                clip.fade_out_samples,
                clip.stretch_backend,
            )
        };
        if clip_muted {
            continue;
        }
        // Resolved at build time (RuntimeProject::resolve_indices) — no id
        // lookup on the audio thread.
        let Some(track_index) = clip_track_index.filter(|&ti| ti < runtime.tracks.len()) else {
            continue;
        };
        if effective_track_muted(&runtime.tracks[track_index], block_beat)
            || (runtime.has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }

        let clip_end = clip_start.saturating_add(clip_duration);
        let mut segment_sample =
            crate::transport::normalize_loop_position(base_sample, loop_bounds);
        let mut callback_offset = 0usize;
        let mut remaining = frames as u64;
        while remaining > 0 {
            let segment_frames = crate::transport::segment_frames_until_loop_wrap(
                segment_sample,
                remaining,
                loop_bounds,
            );
            let block_start = segment_sample;
            let block_end = segment_sample.saturating_add(segment_frames);
            if block_end > clip_start && block_start < clip_end {
                let render_start = clip_start.saturating_sub(block_start) as usize;
                let render_end = (clip_end.min(block_end) - block_start) as usize;
                let segment_render_frames = render_end.saturating_sub(render_start);
                let project_render_start = segment_sample + render_start as u64;
                let rel_start = project_render_start - clip_start;
                if clip_stretch_backend == StretchBackend::Signalsmith
                    && render_signalsmith_clip_segment(
                        runtime,
                        clip_index,
                        track_index,
                        project_render_start,
                        rel_start,
                        callback_offset + render_start,
                        segment_render_frames,
                    )
                {
                    // Rendered through the cached SphereAudioProcessor/Signalsmith
                    // path. Export uses this same render kernel.
                } else {
                    for frame_in_segment in render_start..render_end {
                        let frame_idx = callback_offset + frame_in_segment;
                        let project_sample = segment_sample + frame_in_segment as u64;
                        let rel = project_sample - clip_start;
                        let source_pos_seconds = clip_source_pos_seconds(
                            clip_offset_seconds,
                            rel,
                            clip_duration,
                            runtime.sample_rate,
                            if matches!(clip_processor, ClipDspProcessor::PhaseVocoderBasic) {
                                1.0 / clip_effective_time_ratio.max(0.01)
                            } else {
                                clip_source_read_rate
                            },
                            clip_reverse,
                        );
                        let source_pos = source_pos_seconds * source.sample_rate() as f64;
                        let dry_pos_seconds = clip_source_pos_seconds(
                            clip_offset_seconds,
                            rel,
                            clip_duration,
                            runtime.sample_rate,
                            clip_source_read_rate,
                            clip_reverse,
                        );
                        let dry_source_pos = dry_pos_seconds * source.sample_rate() as f64;
                        let (mut l, mut r) = sample_clip_processor_stereo(
                            &source,
                            source_pos,
                            dry_source_pos,
                            clip_effective_time_ratio,
                            clip_processor,
                        );
                        let fade = clip_fade_gain(rel, clip_duration, clip_fade_in, clip_fade_out);
                        let g = clip_gain * fade;
                        l *= g;
                        r *= g;
                        runtime.tracks[track_index].block_l[frame_idx] += l;
                        runtime.tracks[track_index].block_r[frame_idx] += r;
                    }
                }
            }
            callback_offset += segment_frames as usize;
            remaining -= segment_frames;
            if remaining == 0 {
                break;
            }
            segment_sample = crate::transport::advance_loop_position(
                segment_sample,
                segment_frames,
                loop_bounds,
            )
            .0;
        }
    }

    // ── Pass 1: source tracks (audio / midi / instrument) ───────────────
    // Clips → inserts → fader, sum the post-fader signal into the master
    // output, then feed sends into routing-track receive buffers. Routing
    // tracks (bus/return/group) are deferred to Pass 2 so their inputs are complete.
    // Take the precomputed pass order out by move (zero alloc) rather than
    // cloning the Vec every audio block; the loop body never reads it back, and
    // it is restored below. `audio_graph` is otherwise untouched here.
    let pass1_indices = std::mem::take(&mut runtime.audio_graph.pass1_source_indices);
    for &track_index in &pass1_indices {
        if effective_track_muted(&runtime.tracks[track_index], block_beat)
            || (runtime.has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }
        if callback_debug_enabled()
            && !runtime.tracks[track_index].inserts.is_empty()
            && !runtime.tracks[track_index].callback_clip_route_log_done
        {
            runtime.tracks[track_index].callback_clip_route_log_done = true;
            let track_id = runtime.tracks[track_index].id.clone();
            let block_start = base_sample;
            let block_end = base_sample.saturating_add(frames as u64);
            let input_peak_l = runtime.tracks[track_index].block_l[..frames]
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            let input_peak_r = runtime.tracks[track_index].block_r[..frames]
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            let mut clip_count = 0usize;
            let mut overlapping = 0usize;
            let mut first_clip = String::from("none");
            for clip in runtime
                .clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
            {
                let clip_start = clip.start_sample;
                let clip_end = clip.start_sample.saturating_add(clip.duration_samples);
                let overlaps = block_end > clip_start && block_start < clip_end;
                if clip_count == 0 {
                    first_clip = format!(
                        "{} range={}..{} offset={:.3}s gain={:.3} read_rate={:.3} stretch={:.3} backend={:?} overlaps={}",
                        clip.id,
                        clip_start,
                        clip_end,
                        clip.offset_seconds,
                        clip.gain,
                        clip.source_read_rate,
                        clip.effective_time_ratio,
                        clip.stretch_backend,
                        overlaps
                    );
                }
                clip_count += 1;
                if overlaps {
                    overlapping += 1;
                }
            }
            eprintln!(
                "[SphereAudio callback] clipRoute track={} block={}..{} clips={} overlapping={} preInsertPeakL={:.6} preInsertPeakR={:.6} firstClip={}",
                track_id,
                block_start,
                block_end,
                clip_count,
                overlapping,
                input_peak_l,
                input_peak_r,
                first_clip
            );
        }
        process_track_block(
            runtime,
            track_index,
            frames,
            output,
            channels,
            block_beat,
            transport,
        );
    }
    runtime.audio_graph.pass1_source_indices = pass1_indices;

    // ── Pass 2: routing tracks (bus / return / group) ───────────────────
    // Input = the accumulated send receive buffer. Process inserts → fader and
    // sum to the master output. Solo is ignored for routing tracks so soloing
    // a *source* track still lets its send reach the return. Order comes from
    // the precomputed topological sort in `RuntimeAudioGraph`.
    let pass2_indices = std::mem::take(&mut runtime.audio_graph.pass2_routing_indices);
    for &track_index in &pass2_indices {
        if effective_track_muted(&runtime.tracks[track_index], block_beat) {
            continue;
        }
        {
            let track = &mut runtime.tracks[track_index];
            track.block_l[..frames].copy_from_slice(&track.recv_l[..frames]);
            track.block_r[..frames].copy_from_slice(&track.recv_r[..frames]);
        }
        process_track_block(
            runtime,
            track_index,
            frames,
            output,
            channels,
            block_beat,
            transport,
        );
    }
    runtime.audio_graph.pass2_routing_indices = pass2_indices;

    // ── Master bus: apply master track inserts on the summed output ──
    if let Some(m_idx) = master_index {
        let muted = effective_track_muted(&runtime.tracks[m_idx], block_beat)
            || (runtime.has_solo && !runtime.tracks[m_idx].solo);
        if !muted {
            let master = &mut runtime.tracks[m_idx];
            // Copy summed output into master scratch buffer.
            for i in 0..frames {
                let frame = &output[i * channels..i * channels + channels];
                master.block_l[i] = frame[0];
                master.block_r[i] = frame[1];
            }
            apply_track_chain_block(master, frames, false, transport);
            // Write back, accumulate master meter, apply preview mode.
            for i in 0..frames {
                let (l, r) =
                    apply_preview_mode(master.block_l[i], master.block_r[i], master.preview_mode);
                master.meter_peak_l = master.meter_peak_l.max(l.abs());
                master.meter_peak_r = master.meter_peak_r.max(r.abs());
                master.meter_sum_sq_l += l * l;
                master.meter_sum_sq_r += r * r;
                let out = &mut output[i * channels..i * channels + channels];
                out[0] = l;
                out[1] = r;
            }
        }
    }

    // Final master volume + soft-knee limiter (graceful brick-wall instead of
    // a harsh hard clip when the bus is hot).
    for i in 0..frames {
        let out = &mut output[i * channels..i * channels + channels];
        out[0] = crate::dsp::gain::soft_limit(out[0] * master_volume);
        out[1] = crate::dsp::gain::soft_limit(out[1] * master_volume);
    }

    frames as u64
}

/// Schedule one device callback's MIDI events, splitting only the scheduler
/// range at loop boundaries. The graph/bridge render still runs once for the
/// full device block; offsets are absolute within that callback.
///
/// Returns `Some(loop_start)` when the block ended exactly on a loop boundary
/// and the caller should reset MIDI after rendering so the next callback starts
/// from the loop start.
pub fn schedule_midi_render_block(
    runtime: &mut RuntimeProject,
    base_sample: u64,
    frames: u64,
    loop_bounds: Option<crate::transport::LoopBounds>,
) -> Option<u64> {
    if frames == 0 {
        return None;
    }
    let mut segment_sample = crate::transport::normalize_loop_position(base_sample, loop_bounds);
    let mut remaining = frames;
    let mut callback_offset = 0u64;
    let mut end_reset = None;
    while remaining > 0 {
        let segment_frames = crate::transport::segment_frames_until_loop_wrap(
            segment_sample,
            remaining,
            loop_bounds,
        );
        runtime.schedule_midi_block_with_offset(
            segment_sample,
            segment_frames,
            callback_offset.min(u32::MAX as u64) as u32,
        );
        callback_offset = callback_offset.saturating_add(segment_frames);
        remaining -= segment_frames;
        let (next_sample, wrapped) =
            crate::transport::advance_loop_position(segment_sample, segment_frames, loop_bounds);
        if wrapped {
            if remaining > 0 {
                runtime.reset_midi_playback_with_offset(
                    next_sample,
                    callback_offset.min(u32::MAX as u64) as u32,
                );
            } else {
                end_reset = Some(next_sample);
            }
        }
        segment_sample = next_sample;
    }
    end_reset
}

#[inline]
pub fn is_master_output(output: &str) -> bool {
    output.is_empty() || output == "master" || output == "none"
}

#[inline]
pub fn apply_track_chain_at_beat(
    mut l: f32,
    mut r: f32,
    track: &mut RuntimeTrack,
    beat: f64,
) -> (f32, f32) {
    if !track.inserts.is_empty() && !track.callback_insert_log_done {
        track.callback_insert_log_done = true;
        if callback_debug_enabled() {
            eprintln!(
                "[SphereAudio callback] track={} inserts={}",
                track.id,
                track.inserts.len()
            );
        }
    }
    for insert in &mut track.inserts {
        let processed = apply_insert(l, r, insert);
        l = processed.0;
        r = processed.1;
    }
    let automation = track.automation_values_at_beat(beat);
    let volume = automation.volume.unwrap_or(track.volume);
    let pan = automation.pan.unwrap_or(track.pan);
    let (pan_l, pan_r) = pan_gains(pan);
    (l * volume * pan_l, r * volume * pan_r)
}

/// `bridge_enabled` — false on the master-bus chain, which has never routed
/// external-bridge inserts (parity with the old empty sink-map call); true for
/// regular track strips, where each bridge insert uses its build/command-time
/// cached `bridge_sink` (no per-block `HashMap<String, _>` lookup).
pub fn apply_track_chain_block(
    track: &mut RuntimeTrack,
    frames: usize,
    bridge_enabled: bool,
    transport: RuntimeTransportContext,
) {
    if !track.inserts.is_empty() && !track.callback_insert_log_done {
        track.callback_insert_log_done = true;
        if callback_debug_enabled() {
            eprintln!(
                "[SphereAudio callback] track={} inserts={} blockFrames={}",
                track.id,
                track.inserts.len(),
                frames
            );
        }
    }
    let instrument_ix = track.midi_instrument_insert_ix;
    let midi_events = &track.midi_block_events;
    for (ix, insert) in track.inserts.iter_mut().enumerate() {
        let midi = instrument_ix
            .filter(|&i| i == ix)
            .map(|_| midi_events.as_slice());
        if insert.kind_tag == crate::runtime::RuntimeInsertKind::ExternalBridge {
            // Arc clone (refcount bump only) so the sink can be borrowed
            // alongside the &mut insert.
            let bridge_sink = if bridge_enabled {
                insert.bridge_sink.clone()
            } else {
                None
            };
            apply_external_bridge_insert_block(
                &mut track.block_l[..frames],
                &mut track.block_r[..frames],
                insert,
                midi,
                bridge_sink.as_deref(),
                ix,
                transport,
            );
        } else {
            apply_insert_block(
                &mut track.block_l[..frames],
                &mut track.block_r[..frames],
                insert,
                midi,
                transport,
            );
        }
    }
}

fn push_vst3_midi_to_sink(
    sink: &dyn crate::plugin_bridge::PluginBridgeSink,
    events: &[crate::vst3_processor::Vst3MidiEvent],
    instance_id: &str,
) {
    let verbose = crate::runtime::midi_verbose_enabled();
    for ev in events {
        crate::runtime::push_vst3_midi_event_to_sink(sink, ev, instance_id, verbose);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_external_bridge_insert_block(
    block_l: &mut [f32],
    block_r: &mut [f32],
    insert: &mut RuntimeInsert,
    midi_events: Option<&[crate::vst3_processor::Vst3MidiEvent]>,
    bridge_sink: Option<&dyn crate::plugin_bridge::PluginBridgeSink>,
    slot_index: usize,
    transport: RuntimeTransportContext,
) {
    let frames = block_l.len().min(block_r.len());
    if frames == 0 || !insert.enabled {
        return;
    }
    let Some(sink) = bridge_sink else {
        if plugin_restore_debug_enabled() && insert.bridge_missed_blocks == 0 {
            eprintln!(
                "[AudioGraph] processing insert skipped instance={} reason=no_bridge_sink",
                insert.id
            );
        }
        return;
    };
    if plugin_restore_debug_enabled() && insert.bridge_missed_blocks == 0 {
        let input_peak = block_l[..frames]
            .iter()
            .chain(block_r[..frames].iter())
            .fold(0.0f32, |p, s| p.max(s.abs()));
        eprintln!(
            "[BridgeProcess] track=<chain> slot={slot_index} instance={} input_peak={input_peak:.6}",
            insert.id
        );
    }

    // Clip MIDI for bridged plugins is pushed in schedule_midi_block. Preview
    // MIDI is pushed in drain_commands. Non-bridge inserts still use midi_block_events.
    if let Some(events) = midi_events.filter(|e| !e.is_empty()) {
        let verbose = crate::runtime::midi_verbose_enabled();
        if verbose {
            eprintln!(
                "[plugin-dsp-midi-write] instance={} events={}",
                insert.id,
                events.len()
            );
        }
        push_vst3_midi_to_sink(sink, events, &insert.id);
    }

    // `params["role"]` resolved at build time — no params-map read per block.
    let is_effect = insert.bridge_is_effect;

    if is_effect {
        sink.write_input(&block_l[..frames], &block_r[..frames], frames);
    }

    if insert.scratch_l.len() < frames {
        insert.scratch_l.resize(frames, 0.0);
        insert.scratch_r.resize(frames, 0.0);
    }
    let got = sink.read_output(
        &mut insert.scratch_l[..frames],
        &mut insert.scratch_r[..frames],
        frames,
    );

    // Missed-deadline accounting: `read_output` returns 0 when the host has
    // not produced a fresh block (its service thread is stalled behind an
    // editor open/close or a plugin load). The block below then bypasses the
    // insert (effect keeps the dry signal, instrument contributes silence) —
    // stale output is never replayed. A few misses are normal on startup and
    // when resuming from pause, so only log once a stall is established.
    const BRIDGE_MISS_LOG_THRESHOLD: u32 = 8;
    if got == 0 {
        insert.bridge_missed_blocks = insert.bridge_missed_blocks.saturating_add(1);
        if plugin_restore_debug_enabled()
            && (insert.bridge_missed_blocks == 1
                || insert.bridge_missed_blocks == BRIDGE_MISS_LOG_THRESHOLD
                || insert.bridge_missed_blocks.is_multiple_of(1024))
        {
            eprintln!(
                "[Bridge] missed/bypass instance_id={} missed_blocks={}",
                insert.id, insert.bridge_missed_blocks
            );
        }
        // Stall accounting stays in `bridge_missed_blocks`; stderr from the
        // audio callback only exists under the bridge debug flag (realtime
        // rules — stdio can block the callback).
        if bridge_debug_enabled()
            && (insert.bridge_missed_blocks == BRIDGE_MISS_LOG_THRESHOLD
                || insert.bridge_missed_blocks.is_multiple_of(1024))
        {
            if is_effect {
                eprintln!(
                    "[AudioEngine] plugin missed deadline; bypassing to dry signal instance={} missed_blocks={}",
                    insert.id, insert.bridge_missed_blocks
                );
            } else {
                eprintln!(
                    "[VSTi] missed bridge block; output silence instance={} missed_blocks={}",
                    insert.id, insert.bridge_missed_blocks
                );
            }
        }
    } else {
        if plugin_restore_debug_enabled() {
            let out_peak = insert.scratch_l[..got]
                .iter()
                .chain(insert.scratch_r[..got].iter())
                .fold(0.0f32, |p, s| p.max(s.abs()));
            eprintln!(
                "[BridgeProcess] track=<chain> slot={slot_index} instance={} fresh output_peak={out_peak:.6} frames={got}",
                insert.id
            );
        }
        if bridge_debug_enabled() && insert.bridge_missed_blocks >= BRIDGE_MISS_LOG_THRESHOLD {
            if is_effect {
                eprintln!(
                    "[AudioEngine] plugin host recovered instance={} missed_blocks={}",
                    insert.id, insert.bridge_missed_blocks
                );
            } else {
                eprintln!(
                    "[VSTi] recovered after missed blocks={} instance={}",
                    insert.bridge_missed_blocks, insert.id
                );
            }
        }
        insert.bridge_missed_blocks = 0;
    }

    let mut out_peak_l = 0.0f32;
    let mut out_peak_r = 0.0f32;
    if is_effect && got > 0 {
        block_l[..got].copy_from_slice(&insert.scratch_l[..got]);
        block_r[..got].copy_from_slice(&insert.scratch_r[..got]);
        out_peak_l = insert.scratch_l[..got]
            .iter()
            .fold(0.0f32, |p, s| p.max(s.abs()));
        out_peak_r = insert.scratch_r[..got]
            .iter()
            .fold(0.0f32, |p, s| p.max(s.abs()));
    } else if !is_effect {
        for i in 0..got {
            block_l[i] += insert.scratch_l[i];
            block_r[i] += insert.scratch_r[i];
            out_peak_l = out_peak_l.max(insert.scratch_l[i].abs());
            out_peak_r = out_peak_r.max(insert.scratch_r[i].abs());
        }
    }
    if crate::forensic_trace::engine_midi_verbose_enabled()
        && (out_peak_l > 0.0001 || out_peak_r > 0.0001)
    {
        eprintln!(
            "[SphereAudio] external_bridge output_peak_l={:.6} output_peak_r={:.6}",
            out_peak_l, out_peak_r
        );
        eprintln!(
            "[plugin-host-dsp] response_peak_l={:.6} response_peak_r={:.6}",
            out_peak_l, out_peak_r
        );
    }

    // Publish the real transport ProcessContext for this block before kicking
    // the host, so the bridged plugin sees true tempo/position/playing instead
    // of the old hardcoded stub. Wait-free atomic stores.
    sink.set_transport(&transport);

    // Drive the host DSP handshake: MIDI was already pushed to the shared ring.
    if plugin_restore_debug_enabled() && insert.bridge_missed_blocks == 0 {
        eprintln!(
            "[Bridge] request block instance_id={} frames={frames}",
            insert.id
        );
    }
    sink.request_block(frames as u32);
}

#[inline]
pub fn apply_preview_mode(l: f32, r: f32, mode: RuntimePreviewMode) -> (f32, f32) {
    match mode {
        RuntimePreviewMode::Stereo => (l, r),
        RuntimePreviewMode::Mono | RuntimePreviewMode::Mid => {
            let m = (l + r) * 0.5;
            (m, m)
        }
        RuntimePreviewMode::Side => {
            let s = (l - r) * 0.5;
            (s, s)
        }
    }
}

#[inline]
pub fn apply_insert(l: f32, r: f32, insert: &mut RuntimeInsert) -> (f32, f32) {
    if insert.kind_tag == crate::runtime::RuntimeInsertKind::NativePlugin {
        if !insert.enabled {
            if !insert.callback_process_log_done {
                insert.callback_process_log_done = true;
                // params lookup only inside the once-per-insert log branch.
                let format = insert
                    .params
                    .get("format")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                eprintln!(
                    "[SphereAudio callback] insert={} format={} bypass=true beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                    insert.id,
                    format,
                    l.abs(),
                    r.abs(),
                    l.abs(),
                    r.abs()
                );
            }
            return (l, r);
        }
        if let Some(vst3) = insert.vst3.as_mut() {
            let processed = vst3.process_stereo_sample(l, r);
            let (out_l, out_r) = processed.unwrap_or((l, r));
            if !insert.callback_process_log_done {
                insert.callback_process_log_done = true;
                let format = insert
                    .params
                    .get("format")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                eprintln!(
                    "[SphereAudio callback] insert={} format={} processorHandle=0x{:x} bypass=false processOk={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                    insert.id,
                    format,
                    vst3.handle_value(),
                    processed.is_some(),
                    l.abs(),
                    r.abs(),
                    out_l.abs(),
                    out_r.abs()
                );
            }
            return (out_l, out_r);
        }
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            let format = insert
                .params
                .get("format")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            eprintln!(
                "[SphereAudio callback] insert={} format={} processorHandle=0x0 bypass=false processOk=false beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                insert.id,
                format,
                l.abs(),
                r.abs(),
                l.abs(),
                r.abs()
            );
        }
        return (l, r);
    }

    let plugin_id = canonical_plugin_id(&insert.kind);
    process_stereo_sample(
        plugin_id,
        insert.enabled,
        &insert.params,
        &mut insert.dsp,
        l,
        r,
    )
}

pub fn apply_insert_block(
    block_l: &mut [f32],
    block_r: &mut [f32],
    insert: &mut RuntimeInsert,
    midi_events: Option<&[crate::vst3_processor::Vst3MidiEvent]>,
    transport: RuntimeTransportContext,
) {
    if block_l.is_empty() || block_r.is_empty() {
        return;
    }
    if insert.kind_tag != crate::runtime::RuntimeInsertKind::NativePlugin {
        for i in 0..block_l.len().min(block_r.len()) {
            let (l, r) = apply_insert(block_l[i], block_r[i], insert);
            block_l[i] = l;
            block_r[i] = r;
        }
        return;
    }

    // Diagnostic-only: peak folds feed the once-per-insert process log and the
    // silent-block counter; skipped entirely once that log has fired so the
    // steady-state path stays branch + DSP only. The params "format" lookup
    // happens only inside the log branches.
    let diag = !insert.callback_process_log_done;
    let (before_peak_l, before_peak_r) = if diag {
        (
            block_l
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs())),
            block_r
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs())),
        )
    } else {
        (0.0, 0.0)
    };

    if !insert.enabled {
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            let format = insert
                .params
                .get("format")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            eprintln!(
                "[SphereAudio callback] insert={} format={} bypass=true blockFrames={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                insert.id,
                format,
                block_l.len().min(block_r.len()),
                before_peak_l,
                before_peak_r,
                before_peak_l,
                before_peak_r
            );
        }
        return;
    }

    let Some(vst3) = insert.vst3.as_mut() else {
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            let format = insert
                .params
                .get("format")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            eprintln!(
                "[SphereAudio callback] insert={} format={} processorHandle=0x0 bypass=false processOk=false blockFrames={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                insert.id,
                format,
                block_l.len().min(block_r.len()),
                before_peak_l,
                before_peak_r,
                before_peak_l,
                before_peak_r
            );
        }
        return;
    };

    // Guard: if the underlying C++ processor was destroyed (e.g., Arc dropped
    // on another thread racing with this callback), bypass and log once.
    if !vst3.is_processor_valid() {
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            let format = insert
                .params
                .get("format")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            eprintln!(
                "[SphereAudio callback] insert={} format={} processorHandle=0x{:x} INVALID/DESTROYED bypass=true — insert bypassed to prevent use-after-free",
                insert.id, format, vst3.handle_value()
            );
        }
        return;
    }

    let frames = block_l.len().min(block_r.len());
    if insert.scratch_l.len() < frames {
        insert.scratch_l.resize(frames, 0.0);
        insert.scratch_r.resize(frames, 0.0);
    }
    insert.scratch_l[..frames].fill(0.0);
    insert.scratch_r[..frames].fill(0.0);

    // Real transport ProcessContext for this block, immediately before the
    // plugin processes it (same thread, no race with process()).
    vst3.set_process_context(&transport);

    let handle = vst3.handle_value();
    let process_ok = if let Some(events) = midi_events.filter(|e| !e.is_empty()) {
        vst3.process_stereo_block_with_midi(
            &block_l[..frames],
            &block_r[..frames],
            &mut insert.scratch_l[..frames],
            &mut insert.scratch_r[..frames],
            events,
        )
    } else {
        vst3.process_stereo_block(
            &block_l[..frames],
            &block_r[..frames],
            &mut insert.scratch_l[..frames],
            &mut insert.scratch_r[..frames],
        )
    };
    if process_ok {
        block_l[..frames].copy_from_slice(&insert.scratch_l[..frames]);
        block_r[..frames].copy_from_slice(&insert.scratch_r[..frames]);
    }

    if diag && before_peak_l <= 0.000001 && before_peak_r <= 0.000001 {
        insert.silent_process_blocks = insert.silent_process_blocks.saturating_add(1);
    }

    if diag
        && (before_peak_l > 0.000001
            || before_peak_r > 0.000001
            || insert.silent_process_blocks >= 200)
    {
        insert.callback_process_log_done = true;
        let format = insert
            .params
            .get("format")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let after_peak_l = block_l[..frames]
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        let after_peak_r = block_r[..frames]
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        eprintln!(
            "[SphereAudio callback] insert={} format={} processorHandle=0x{:x} bypass=false processOk={} blockFrames={} silentBlocks={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
            insert.id,
            format,
            handle,
            process_ok,
            frames,
            insert.silent_process_blocks,
            before_peak_l,
            before_peak_r,
            after_peak_l,
            after_peak_r
        );
    }
}

#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    if pan < 0.0 {
        (1.0, 1.0 + pan)
    } else {
        (1.0 - pan, 1.0)
    }
}
