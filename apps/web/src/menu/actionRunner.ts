import type { DawTrack } from "../types/daw";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useTransportStore } from "../store/transportStore";
import { useMetronomeStore } from "../store/metronomeStore";
import { useHistoryStore } from "../store/historyStore";
import { transport } from "../engine/Transport";
import { getTrackColor } from "../theme";
import { platform } from "../platform";
import { importAudioFilesAsNewTracks } from "../utils/importAudioToProject";
import { midiEditorBridge } from "./midiEditorBridge";
import {
  AddTrackCommand,
  DeleteTrackCommand,
  DeleteClipsCommand,
  DuplicateClipsCommand,
  DuplicateTrackCommand,
  RenameTrackCommand,
  SetTrackColorCommand,
  SetTrackMuteCommand,
  SetTrackSoloCommand,
} from "../commands";

export function runAction(actionId: string) {
  // Close command palette if open
  const uiStore = useUIStore.getState();
  if (uiStore.commandPaletteOpen) uiStore.setCommandPaletteOpen(false);

  const projectStore = useProjectStore.getState();
  const transportStore = useTransportStore.getState();
  const metronomeStore = useMetronomeStore.getState();
  const history = useHistoryStore.getState();

  switch (actionId) {
    // ── Tools ──────────────────────────────────────────────────────────────
    case "tools:command-palette":
    case "tools:quick-search":
      uiStore.toggleCommandPalette();
      break;

    case "command:close":
      uiStore.setCommandPaletteOpen(false);
      break;

    // ── Transport ──────────────────────────────────────────────────────────
    case "transport:play-pause":
      if (transportStore.isPlaying) {
        transport.pause();
        transportStore.setIsPlaying(false);
      } else {
        void transport.play().then(() => transportStore.setIsPlaying(true));
      }
      break;

    case "transport:stop":
      transport.stop();
      transportStore.setIsPlaying(false);
      break;

    case "transport:go-to-start":
      transport.seek(0);
      break;

    case "transport:toggle-loop":
      uiStore.toggleLoop();
      break;

    case "transport:toggle-metronome":
      metronomeStore.toggle();
      break;

    case "transport:toggle-count-in":
      metronomeStore.toggleCountIn();
      break;

    // ── Edit ───────────────────────────────────────────────────────────────
    case "edit:undo":
      history.undo();
      break;

    case "edit:redo":
      history.redo();
      break;

    case "edit:delete": {
      const { selectedClipIds, selectedTrackId, focusedPanel } = uiStore;
      if (focusedPanel === "timeline" && selectedClipIds.length > 0) {
        history.execute(new DeleteClipsCommand(selectedClipIds));
        uiStore.setSelectedClipIds([]);
      } else if (selectedTrackId) {
        history.execute(new DeleteTrackCommand(selectedTrackId));
        uiStore.setSelectedTrackId(null);
        uiStore.setSelectedMixerTrackId(null);
      }
      break;
    }

    case "edit:delete-track": {
      const { selectedTrackId } = uiStore;
      if (selectedTrackId) {
        history.execute(new DeleteTrackCommand(selectedTrackId));
        uiStore.setSelectedTrackId(null);
        uiStore.setSelectedMixerTrackId(null);
      }
      break;
    }

    case "edit:duplicate": {
      const { selectedClipIds } = uiStore;
      if (selectedClipIds.length > 0) history.execute(new DuplicateClipsCommand(selectedClipIds));
      break;
    }

    case "edit:deselect-all":
      uiStore.setSelectedClipIds([]);
      uiStore.setSelectedTrackId(null);
      break;

    case "timeline:toggle-snap":
      uiStore.toggleSnapToGrid();
      break;

    // ── Track context-menu actions ─────────────────────────────────────────
    case "track:rename": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (!track) break;
      const newName = window.prompt("Rename track:", track.name)?.trim();
      if (newName && newName !== track.name) {
        history.execute(new RenameTrackCommand(selectedTrackId, newName, track.name));
      }
      break;
    }

    case "track:duplicate": {
      const { selectedTrackId } = uiStore;
      if (selectedTrackId) history.execute(new DuplicateTrackCommand(selectedTrackId));
      break;
    }

    case "track:toggle-mute": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (track) history.execute(new SetTrackMuteCommand(selectedTrackId, !track.muted));
      break;
    }

    case "track:toggle-solo": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (track) history.execute(new SetTrackSoloCommand(selectedTrackId, !track.solo));
      break;
    }

    case "track:toggle-arm": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (track) projectStore.setTrackArmed(selectedTrackId, !track.armed);
      break;
    }

    case "track:add-audio": {
      const tracks = projectStore.project.tracks;
      const newId = crypto.randomUUID();
      const newTrack: DawTrack = {
        id: newId,
        name: `Audio Track ${tracks.length + 1}`,
        type: "audio",
        color: getTrackColor(tracks.length),
        channelCount: 2,
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
      };
      history.execute(new AddTrackCommand(newTrack));
      uiStore.setSelectedTrackId(newId);
      break;
    }

    // ── MIDI editor actions (routed through bridge to active panel) ───────────
    case "midi:select-all":
      midiEditorBridge.call("selectAll");
      break;

    case "midi:delete-selected":
      midiEditorBridge.call("deleteSelected");
      break;

    case "midi:duplicate-selected":
      midiEditorBridge.call("duplicateSelected");
      break;

    case "midi:quantize":
      midiEditorBridge.call("quantize");
      break;

    case "midi:nudge-left":
      midiEditorBridge.call("nudgeLeft");
      break;

    case "midi:nudge-right":
      midiEditorBridge.call("nudgeRight");
      break;

    case "midi:transpose-up":
      midiEditorBridge.call("transposeUp");
      break;

    case "midi:transpose-down":
      midiEditorBridge.call("transposeDown");
      break;

    case "midi:transpose-octave-up":
      midiEditorBridge.call("transposeOctaveUp");
      break;

    case "midi:transpose-octave-down":
      midiEditorBridge.call("transposeOctaveDown");
      break;

    case "track:add-bus": {
      const tracks = projectStore.project.tracks;
      const id = crypto.randomUUID();
      const newTrack: DawTrack = {
        id,
        name: `Bus ${tracks.filter((t) => t.type === "bus").length + 1}`,
        type: "bus",
        color: getTrackColor(tracks.length),
        channelCount: 2, volume: 1, pan: 0, muted: false, solo: false, armed: false,
        clips: [], output: "master", sends: [], inserts: [],
      };
      history.execute(new AddTrackCommand(newTrack));
      uiStore.setSelectedTrackId(id);
      break;
    }

    case "track:add-return": {
      const tracks = projectStore.project.tracks;
      const id = crypto.randomUUID();
      const newTrack: DawTrack = {
        id,
        name: `Return ${tracks.filter((t) => t.type === "return").length + 1}`,
        type: "return",
        color: getTrackColor(tracks.length),
        channelCount: 2, volume: 1, pan: 0, muted: false, solo: false, armed: false,
        clips: [], output: "master", sends: [], inserts: [],
      };
      history.execute(new AddTrackCommand(newTrack));
      uiStore.setSelectedTrackId(id);
      break;
    }

    case "track:add-group": {
      const tracks = projectStore.project.tracks;
      const id = crypto.randomUUID();
      const newTrack: DawTrack = {
        id,
        name: `Group ${tracks.filter((t) => t.type === "group").length + 1}`,
        type: "group",
        color: getTrackColor(tracks.length),
        channelCount: 2, volume: 1, pan: 0, muted: false, solo: false, armed: false,
        clips: [], output: "master", sends: [], inserts: [],
      };
      history.execute(new AddTrackCommand(newTrack));
      uiStore.setSelectedTrackId(id);
      break;
    }

    // Stubs — not yet implemented
    case "track:add-midi":
    case "track:add-plugin":
    case "track:freeze":
    case "track:flatten":
    case "track:route-to":
    case "track:settings":
      break;

    // ── View ───────────────────────────────────────────────────────────────
    case "panel:toggle-mixer":
    case "view:toggle-mixer":
    case "window.show_mixer":
      uiStore.togglePanel("mixer");
      break;

    case "panel:toggle-inspector":
    case "view:toggle-inspector":
    case "window.show_inspector":
      uiStore.togglePanel("inspector");
      break;

    // ── Project ────────────────────────────────────────────────────────────
    case "project:save":
      void platform.projectStorage
        .saveProject(useProjectStore.getState().project)
        .catch((e) => console.warn("[ActionRunner] save project:", e));
      break;

    case "project:open":
      void platform.projectStorage
        .openProject()
        .then((p) => {
          if (p) useProjectStore.setState({ project: p });
        })
        .catch((e) => console.warn("[ActionRunner] open project:", e));
      break;

    // ── File ───────────────────────────────────────────────────────────────
    case "file:import-audio":
      void platform.fileSystem
        .pickAudioFiles()
        .then((files) => {
          if (files.length > 0) return importAudioFilesAsNewTracks(files);
        })
        .catch((e) => console.warn("[ActionRunner] import audio:", e));
      break;

    // file:reveal-in-folder is also handled via the dynamic prefix in the default branch
    // so future UI can pass a path (e.g. `file:reveal-in-folder:<path>`).
    case "file:reveal-in-folder":
      break;

    // ── Window ─────────────────────────────────────────────────────────────
    case "window:minimize":
      platform.window.minimize();
      break;

    case "window:maximize":
    case "window:toggle-maximize":
      platform.window.toggleMaximize();
      break;

    case "window:close":
      platform.window.close();
      break;

    case "noop":
      break;

    default:
      // file:reveal-in-folder:<path> — show item in OS file manager.
      if (actionId.startsWith("file:reveal-in-folder:")) {
        if (!platform.capabilities.filesystem) break;
        const path = actionId.slice("file:reveal-in-folder:".length);
        if (path) {
          void platform.fileSystem
            .revealInFileManager(path)
            .catch((e) => console.warn("[ActionRunner] reveal:", e));
        }
        break;
      }
      // track:color:#RRGGBB
      if (actionId.startsWith("track:color:")) {
        const color = actionId.slice("track:color:".length);
        const { selectedTrackId } = uiStore;
        if (selectedTrackId) {
          const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
          if (track) history.execute(new SetTrackColorCommand(selectedTrackId, color, track.color));
        }
        break;
      }
      console.warn(`[ActionRunner] Unhandled action: ${actionId}`);
  }
}
