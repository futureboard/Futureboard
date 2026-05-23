import { useEffect } from "react";
import "../App.css";
import { AddTrackDialog } from "../components/AddTrackDialog";
import { useProjectStore } from "../store/projectStore";
import { platform } from "../platform";
import { audioDeviceService } from "../engine/AudioDeviceService";
import { midiDeviceService } from "../engine/MidiDeviceService";

export function ExternalAddTrackWindow() {
  const loadLocal = useProjectStore((s) => s.loadLocal);

  useEffect(() => {
    loadLocal();
  }, [loadLocal]);

  useEffect(() => {
    if (platform.kind === "electron") {
      void audioDeviceService.refreshAudioDevices();
      void midiDeviceService.requestMidiAccess().catch(() => {
        midiDeviceService.refreshMidiDevices();
      });
    } else {
      void audioDeviceService.refreshAudioDevices();
      midiDeviceService.refreshMidiDevices();
    }
  }, []);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-[#1a1e26] text-daw-text">
      <AddTrackDialog
        onClose={() => window.close()}
        external
      />
    </div>
  );
}
