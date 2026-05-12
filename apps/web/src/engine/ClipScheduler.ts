import type { DawClip, DawTrack } from "../types/daw";
import { audioEngine } from "./AudioEngine";
import { mixer } from "./Mixer";
import { transport } from "./Transport";

type ScheduledSource = {
  node: AudioBufferSourceNode;
  clipId: string;
};

class ClipScheduler {
  private scheduled: ScheduledSource[] = [];

  schedule(tracks: DawTrack[]) {
    this.cancelAll();

    const playheadTime = transport.projectTime;
    const audioNow = audioEngine.currentTime;

    for (const track of tracks) {
      const trackInput = mixer.getOrCreateTrack(track.id, track.volume, track.pan).gain;

      for (const clip of track.clips) {
        const clipEnd = clip.startTime + clip.duration;
        if (clipEnd <= playheadTime) continue;

        const loaded = audioEngine.getBuffer(clip.fileId);
        if (!loaded) continue;

        const node = audioEngine.ctx.createBufferSource();
        node.buffer = loaded.audioBuffer;

        const gainNode = audioEngine.ctx.createGain();
        gainNode.gain.value = clip.gain;
        node.connect(gainNode);
        gainNode.connect(trackInput);

        let clipOffset: number;
        let scheduleAt: number;

        if (clip.startTime >= playheadTime) {
          // Clip starts after playhead — schedule it in the future
          clipOffset = clip.offset;
          scheduleAt = audioNow + (clip.startTime - playheadTime);
        } else {
          // Playhead is inside the clip — start from correct offset
          clipOffset = clip.offset + (playheadTime - clip.startTime);
          scheduleAt = audioNow;
        }

        const remainingDuration = clip.duration - (clipOffset - clip.offset);
        if (remainingDuration <= 0) continue;

        node.start(scheduleAt, clipOffset, remainingDuration);
        this.scheduled.push({ node, clipId: clip.id });
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
