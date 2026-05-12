import { useEffect, useRef } from "react";
import { AppShell } from "./components/AppShell";
import { TransportBar } from "./components/TransportBar";
import { audioEngine } from "./engine/AudioEngine";
import { mixer } from "./engine/Mixer";
import { useProjectStore } from "./store/projectStore";
import { getTrackColor } from "./theme";
import type { DawClip, DawFile, DawTrack } from "./types/daw";
import "./App.css";

export default function App() {
  const { addTrack, addFile, addClip, setPeaks, loadLocal, saveLocal, project } = useProjectStore();
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    loadLocal();
  }, []);

  const handleImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files || files.length === 0) return;

    for (const f of Array.from(files)) {
      const arrayBuffer = await f.arrayBuffer();
      const fileId = crypto.randomUUID();
      const trackId = crypto.randomUUID();
      const clipId = crypto.randomUUID();
      const trackColor = getTrackColor(project.tracks.length);

      try {
        const audioBuffer = await audioEngine.loadBuffer(
          { id: fileId, name: f.name, mimeType: f.type, duration: 0, sampleRate: 48000, channels: 2 },
          arrayBuffer,
          (fid, peaks) => setPeaks(fid, peaks)
        );

        const dawFile: DawFile = {
          id: fileId,
          name: f.name,
          mimeType: f.type,
          duration: audioBuffer.duration,
          sampleRate: audioBuffer.sampleRate,
          channels: audioBuffer.numberOfChannels,
          localObjectUrl: URL.createObjectURL(f),
        };

        const track: DawTrack = {
          id: trackId,
          name: f.name.replace(/\.[^.]+$/, ""),
          type: "audio",
          color: trackColor,
          volume: 0.8,
          pan: 0,
          muted: false,
          solo: false,
          armed: false,
          clips: [],
        };

        const clip: DawClip = {
          id: clipId,
          name: f.name.replace(/\.[^.]+$/, ""),
          fileId,
          trackId,
          startTime: 0,
          offset: 0,
          duration: audioBuffer.duration,
          gain: 1,
        };

        mixer.getOrCreateTrack(trackId, track.volume, track.pan);
        addFile(dawFile);
        addTrack(track);
        addClip(trackId, clip);
      } catch (err) {
        console.error("Failed to import", f.name, err);
        alert(`Could not import "${f.name}". The format may not be supported.`);
      }
    }

    if (fileInputRef.current) fileInputRef.current.value = "";
  };

  return (
    <div className="flex h-full flex-col bg-daw-bg text-daw-text">
      <input
        ref={fileInputRef}
        id="audio-import"
        type="file"
        accept=".wav,.mp3,audio/wav,audio/mpeg"
        multiple
        style={{ display: "none" }}
        onChange={handleImport}
      />

      <TransportBar
        onImport={() => fileInputRef.current?.click()}
        onSave={saveLocal}
      />

      <div className="min-h-0 flex-1 overflow-hidden">
        <AppShell onImport={() => fileInputRef.current?.click()} />
      </div>
    </div>
  );
}
