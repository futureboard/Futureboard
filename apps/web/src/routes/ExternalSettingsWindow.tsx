import { useEffect, useMemo } from "react";
import "../App.css";
import { SettingsDialog } from "../components/settings/SettingsDialog";
import { audioDeviceService } from "../engine/AudioDeviceService";
import { midiDeviceService } from "../engine/MidiDeviceService";
import { platform } from "../platform";

type SettingsTab = "general" | "audio" | "midi" | "project" | "library" | "shortcuts" | "appearance" | "advanced";
const VALID_TABS = new Set<SettingsTab>(["general", "audio", "midi", "project", "library", "shortcuts", "appearance", "advanced"]);

export function ExternalSettingsWindow() {
  const initialTab = useMemo<SettingsTab>(() => {
    const params = new URLSearchParams(window.location.hash.split("?")[1] ?? "");
    const tab = params.get("tab") as SettingsTab | null;
    return tab && VALID_TABS.has(tab) ? tab : "general";
  }, []);

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
    audioDeviceService.listenForDeviceChanges();
    return () => {
      audioDeviceService.stopListening();
    };
  }, []);

  return (
    <div className="h-screen w-screen overflow-hidden bg-[#0e1319] text-daw-text">
      <SettingsDialog windowId="preferences" initialTab={initialTab} external />
    </div>
  );
}
