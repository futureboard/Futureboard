import type { DawTrack } from "../types/daw";
import { audioEngine } from "./AudioEngine";
import { mixer } from "./Mixer";
import { transport } from "./Transport";
import { audioCacheManager } from "../audio/AudioCacheManager";
import { audioProcessingService } from "../audio/AudioProcessingService";
import { decodedAudioToAudioBuffer } from "../audio/audioCacheTypes";
import type { AudioProcessParams } from "../audio/audioCacheTypes";
import { buildDecodedCacheKey } from "../audio/audioCacheKeys";
import { useProjectStore } from "../store/projectStore";

type ScheduledSource = {
  node:     AudioBufferSourceNode;
  effectNode?: AudioNode;
  gainNode: GainNode;
  clipId:   string;
  clipGain: number; // stored so unmute can restore correct level
};

class ClipScheduler {
  private scheduled: ScheduledSource[] = [];

  schedule(tracks: DawTrack[]) {
    this.cancelAll();

    const playheadTime = transport.projectTime;
    const audioNow     = audioEngine.currentTime;
    let scheduledCount = 0;

    for (const track of tracks) {
      const trackInput = mixer.getOrCreateTrack(track.id, track.volume, track.pan).gain;

      // Sync phase invert from project state every time we reschedule.
      mixer.setPhaseInvert(track.id, track.advanced?.phaseInvert ?? false);

      // Track-level playback delay offset (positive = shift later, negative clamped to 0).
      const trackDelayS = Math.max(0, (track.advanced?.delayMs ?? 0) / 1000);

      for (const clip of track.clips) {
        const clipEnd = clip.startTime + clip.duration;
        if (clipEnd <= playheadTime) continue;

        const loaded = audioEngine.getBuffer(clip.fileId);
        if (!loaded) {
          const file = useProjectStore.getState().project.files.find((f) => f.id === clip.fileId);
          if (file) {
            void audioEngine.ensureBuffer(file).then((buffer) => {
              if (buffer && transport.isPlaying) this.schedule(useProjectStore.getState().project.tracks);
            });
          }
          continue;
        }

        // Resolve the AudioBuffer to play — use processed version if available.
        let audioBuffer: AudioBuffer | null = loaded.audioBuffer;
        let bufferTimeScale = 1;
        let realtimeRate    = 1;
        let soundTouchParams: AudioProcessParams | null = null;

        if (clip.audioProcess) {
          const proc = clip.audioProcess;
          const hasEffect =
            proc.speedRatio !== 1 || proc.pitchSemitones !== 0;

          if (hasEffect) {
            const params: AudioProcessParams = {
              speedRatio:     proc.speedRatio,
              pitchSemitones: proc.pitchSemitones,
              preservePitch:  proc.preservePitch,
              mode:           proc.mode ?? "polyphonic",
              quality:        proc.quality,
            };
            const decoded = audioCacheManager.getDecodedAudio(
              buildDecodedCacheKey(clip.fileId, loaded.audioBuffer.sampleRate),
            );
            if (decoded) {
              const processed = audioProcessingService.getCachedProcessed(decoded, params);
              if (processed) {
                audioBuffer = decodedAudioToAudioBuffer(audioEngine.ctx, processed);
                bufferTimeScale = proc.speedRatio;
              } else if (canUseSoundTouch(params)) {
                soundTouchParams = params;
              } else {
                realtimeRate = realtimePlaybackRate(params);
              }
              // If not cached yet, fall through to original buffer.
            } else {
              if (canUseSoundTouch(params)) {
                soundTouchParams = params;
              } else {
                realtimeRate = realtimePlaybackRate(params);
              }
            }
          }
        }

        const clipMuted = clip.muted ?? false;
        const effectiveGain = clipMuted ? 0 : clip.gain;

        const gainNode = audioEngine.ctx.createGain();
        gainNode.gain.value = effectiveGain;

        let clipOffset: number;
        let scheduleAt: number;

        if (clip.startTime >= playheadTime) {
          clipOffset = clip.offset / bufferTimeScale;
          scheduleAt = audioNow + (clip.startTime - playheadTime) + trackDelayS;
        } else {
          // Playhead is inside the clip — scale offset into processed buffer time.
          const rawOffset = clip.offset + (playheadTime - clip.startTime);
          clipOffset      = rawOffset / bufferTimeScale;
          scheduleAt      = Math.max(audioNow, audioNow + trackDelayS);
        }

        const outputTimeScale = soundTouchParams ? soundTouchParams.speedRatio : bufferTimeScale * realtimeRate;
        const remainingDuration =
          (clip.duration - (clipOffset * bufferTimeScale - clip.offset)) / outputTimeScale;
        if (
          remainingDuration <= 0 ||
          !Number.isFinite(remainingDuration) ||
          !Number.isFinite(clipOffset) ||
          !Number.isFinite(scheduleAt)
        ) {
          console.warn("[WebAudio] scheduling clip skipped: invalid timing", {
            clipId: clip.id,
            clipOffset,
            scheduleAt,
            remainingDuration,
          });
          continue;
        }

        let effectNode: AudioNode | undefined;
        const source = audioEngine.ctx.createBufferSource();
        source.buffer = audioBuffer ?? loaded.audioBuffer;

        if (soundTouchParams) {
          try {
            const soundTouch = audioEngine.createSoundTouchNode();
            source.playbackRate.value = soundTouchParams.speedRatio;
            soundTouch.playbackRate.value = soundTouchParams.speedRatio;
            soundTouch.pitch.value = 1;
            soundTouch.pitchSemitones.value = soundTouchParams.pitchSemitones;
            soundTouch.setStretchParameters({ overlapMs: 12, quickSeek: true });
            source.connect(soundTouch);
            soundTouch.connect(gainNode);
            effectNode = soundTouch;
          } catch (error) {
            console.warn("[ClipScheduler] SoundTouch unavailable, falling back to playbackRate:", error);
            source.playbackRate.value = realtimePlaybackRate(soundTouchParams);
            source.connect(gainNode);
          }
        } else {
          source.playbackRate.value = realtimeRate;
          source.connect(gainNode);
        }

        gainNode.connect(trackInput);
        console.log("[WebAudio] scheduling clip", {
          clipId: clip.id,
          trackId: track.id,
          scheduleAt,
          clipOffset,
          remainingDuration,
        });
        source.start(scheduleAt, clipOffset, remainingDuration);
        console.log("[WebAudio] source started", { clipId: clip.id });
        this.scheduled.push({ node: source, effectNode, gainNode, clipId: clip.id, clipGain: clip.gain });
        scheduledCount++;
      }
    }
    console.log("[WebAudio] output signal/meter", { scheduledCount });
  }

  /**
   * Update the gain of a currently-playing clip source node in realtime.
   * Safe to call while playback is running — uses setTargetAtTime for smooth ramp.
   */
  updateClipGain(clipId: string, gain: number): void {
    for (const s of this.scheduled) {
      if (s.clipId === clipId) {
        s.clipGain = gain;
        s.gainNode.gain.setTargetAtTime(gain, audioEngine.currentTime, 0.01);
      }
    }
  }

  /**
   * Mute or unmute a currently-playing clip source node in realtime.
   * Restores the last known clipGain when unmuting.
   */
  updateClipMute(clipId: string, muted: boolean): void {
    for (const s of this.scheduled) {
      if (s.clipId === clipId) {
        const target = muted ? 0 : s.clipGain;
        s.gainNode.gain.setTargetAtTime(target, audioEngine.currentTime, 0.01);
      }
    }
  }

  cancelAll() {
    for (const { node, effectNode } of this.scheduled) {
      try {
        node.stop();
        node.disconnect();
        effectNode?.disconnect();
      } catch {
        // already stopped
      }
    }
    this.scheduled = [];
  }
}

export const clipScheduler = new ClipScheduler();

function realtimePlaybackRate(params: AudioProcessParams): number {
  let rate = params.speedRatio;
  if (params.pitchSemitones !== 0) {
    rate *= Math.pow(2, params.pitchSemitones / 12);
  }
  return Math.max(0.25, Math.min(4.0, rate));
}

function canUseSoundTouch(params: AudioProcessParams): boolean {
  return params.preservePitch
    && params.mode !== "resample"
    && (params.pitchSemitones !== 0 || params.speedRatio !== 1);
}
