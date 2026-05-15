import type { DawTrack } from "../types/daw";
import { audioEngine } from "./AudioEngine";
import { mixer } from "./Mixer";
import { transport } from "./Transport";
import { audioCacheManager } from "../audio/AudioCacheManager";
import { audioProcessingService } from "../audio/AudioProcessingService";
import { decodedAudioToAudioBuffer } from "../audio/audioCacheTypes";
import type { AudioProcessParams } from "../audio/audioCacheTypes";
import { buildDecodedCacheKey } from "../audio/audioCacheKeys";

type ScheduledSource = {
  node:     AudioBufferSourceNode;
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

    for (const track of tracks) {
      const trackInput = mixer.getOrCreateTrack(track.id, track.volume, track.pan).gain;

      for (const clip of track.clips) {
        const clipEnd = clip.startTime + clip.duration;
        if (clipEnd <= playheadTime) continue;

        const loaded = audioEngine.getBuffer(clip.fileId);
        if (!loaded) continue;

        // Resolve the AudioBuffer to play — use processed version if available.
        let audioBuffer = loaded.audioBuffer;
        let speedRatio  = 1;

        if (clip.audioProcess) {
          const proc = clip.audioProcess;
          const hasEffect =
            proc.speedRatio !== 1 || proc.pitchSemitones !== 0;

          if (hasEffect) {
            const decoded = audioCacheManager.getDecodedAudio(
              buildDecodedCacheKey(clip.fileId, loaded.audioBuffer.sampleRate),
            );
            if (decoded) {
              const params: AudioProcessParams = {
                speedRatio:     proc.speedRatio,
                pitchSemitones: proc.pitchSemitones,
                preservePitch:  proc.preservePitch,
                mode:           proc.mode ?? "polyphonic",
                quality:        proc.quality,
              };
              const processed = audioProcessingService.getCachedProcessed(decoded, params);
              if (processed) {
                audioBuffer = decodedAudioToAudioBuffer(audioEngine.ctx, processed);
                speedRatio  = proc.speedRatio;
              }
              // If not cached yet, fall through to original buffer.
            }
          }
        }

        const clipMuted = clip.muted ?? false;
        const effectiveGain = clipMuted ? 0 : clip.gain;

        const node     = audioEngine.ctx.createBufferSource();
        node.buffer    = audioBuffer;

        const gainNode = audioEngine.ctx.createGain();
        gainNode.gain.value = effectiveGain;
        node.connect(gainNode);
        gainNode.connect(trackInput);

        let clipOffset: number;
        let scheduleAt: number;

        if (clip.startTime >= playheadTime) {
          clipOffset = clip.offset / speedRatio;
          scheduleAt = audioNow + (clip.startTime - playheadTime);
        } else {
          // Playhead is inside the clip — scale offset into processed buffer time.
          const rawOffset = clip.offset + (playheadTime - clip.startTime);
          clipOffset      = rawOffset / speedRatio;
          scheduleAt      = audioNow;
        }

        const remainingDuration =
          (clip.duration - (clipOffset * speedRatio - clip.offset)) / speedRatio;
        if (remainingDuration <= 0) continue;

        node.start(scheduleAt, clipOffset, remainingDuration);
        this.scheduled.push({ node, gainNode, clipId: clip.id, clipGain: clip.gain });
      }
    }
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
    for (const { node } of this.scheduled) {
      try {
        node.stop();
        node.disconnect();
      } catch {
        // already stopped
      }
    }
    this.scheduled = [];
  }
}

export const clipScheduler = new ClipScheduler();
