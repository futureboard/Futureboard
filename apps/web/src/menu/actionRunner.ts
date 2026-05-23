import type { DawTrack, TrackPreviewMode } from "../types/daw";
import { useProjectStore } from "../store/projectStore";
import { createTrackVolumeTarget, createTrackPanTarget } from "../utils/automationTargets";
import { useUIStore } from "../store/uiStore";
import { useTransportStore } from "../store/transportStore";
import { useMetronomeStore } from "../store/metronomeStore";
import { useHistoryStore } from "../store/historyStore";
import { useRecentProjectsStore } from "../store/recentProjectsStore";
import { activeAudioEngine } from "../engine/activeAudioEngine";
import { getTrackColor } from "../theme";
import { platform } from "../platform";
import { importAudioFilesAsNewTracks } from "../utils/importAudioToProject";
import { midiEditorBridge } from "./midiEditorBridge";
import { audioDeviceService } from "../engine/AudioDeviceService";
import { midiDeviceService } from "../engine/MidiDeviceService";
import { showToast } from "../components/ui/Toast";
import type { SnapDivision } from "../utils/musicalTime";
import { buildSelectionState, getActiveSelectionContext } from "../store/selectionSelectors";
import {
  guardUnsavedProject,
  loadOpenedProject,
  openProjectFromPath,
  rememberSavedProject,
  saveCurrentProjectAndRemember,
} from "../utils/projectLifecycle";
import { openAddTrackWindow, openPluginManagerWindow, openProjectWizardWindow, openSettingsWindow } from "../utils/dialogWindows";
import {
  AddTrackCommand,
  DeleteTrackCommand,
  DeleteClipsCommand,
  DuplicateClipsCommand,
  DuplicateTrackCommand,
  RenameTrackCommand,
  SetTrackColorCommand,
  SetTrackMuteCommand,
  SetTrackPanCommand,
  SetTrackPreviewModeCommand,
  SetTrackSoloCommand,
  SetTrackVolumeCommand,
  SplitClipCommand,
} from "../commands";

export function runAction(actionId: string) {
  const uiStore = useUIStore.getState();
  if (uiStore.commandPaletteOpen) uiStore.setCommandPaletteOpen(false);

  const projectStore = useProjectStore.getState();
  const transportStore = useTransportStore.getState();
  const metronomeStore = useMetronomeStore.getState();
  const history = useHistoryStore.getState();

  switch (actionId) {
    // ── Command palette ────────────────────────────────────────────────────
    case "tools:command-palette":
    case "tools:quick-search":
      uiStore.toggleCommandPalette();
      break;

    case "command:close":
      uiStore.setCommandPaletteOpen(false);
      break;

    // ── Arrangement tools ──────────────────────────────────────────────────
    case "tools:select-pointer":
      uiStore.setCurrentTool("pointer");
      break;
    case "tools:select-pen":
      uiStore.setCurrentTool("pen");
      break;
    case "tools:select-cut":
      uiStore.setCurrentTool("cut");
      break;
    case "tools:select-glue":
      uiStore.setCurrentTool("glue");
      break;
    case "tools:select-mute":
      uiStore.setCurrentTool("mute");
      break;
    case "tools:select-time":
      uiStore.setCurrentTool("time");
      break;
    case "tools:select-automation":
      uiStore.setCurrentTool("automation");
      break;

    // ── Transport ──────────────────────────────────────────────────────────
    case "transport:play-pause":
      if (transportStore.isPlaying) {
        activeAudioEngine.pause();
        transportStore.setIsPlaying(false);
      } else {
        void activeAudioEngine.play().then(() => transportStore.setIsPlaying(true));
      }
      break;

    case "transport:stop":
      activeAudioEngine.stop();
      transportStore.setIsPlaying(false);
      break;

    case "transport:go-to-start":
      activeAudioEngine.seekSeconds(0);
      break;

    case "transport:go-to-end": {
      const { tracks } = projectStore.project;
      const end = tracks.reduce((max, track) => {
        const trackEnd = track.clips.reduce((m, c) => Math.max(m, c.startTime + c.duration), 0);
        return Math.max(max, trackEnd);
      }, 0);
      activeAudioEngine.seekSeconds(end);
      break;
    }

    case "transport:rewind": {
      const { bpm, timeSignature } = projectStore.project;
      const timeSig = timeSignature ?? { numerator: 4, denominator: 4 };
      const barLen = (60 / bpm) * timeSig.numerator;
      activeAudioEngine.seekSeconds(Math.max(0, activeAudioEngine.projectTime - barLen));
      break;
    }

    case "transport:fast-forward": {
      const { bpm, timeSignature } = projectStore.project;
      const timeSig = timeSignature ?? { numerator: 4, denominator: 4 };
      const barLen = (60 / bpm) * timeSig.numerator;
      activeAudioEngine.seekSeconds(activeAudioEngine.projectTime + barLen);
      break;
    }

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

    // clipboard not yet implemented
    case "edit:cut":
    case "edit:copy":
    case "edit:paste":
      break;

    case "edit:delete": {
      const sel = buildSelectionState(uiStore);
      const ctx = getActiveSelectionContext(sel);
      if (ctx.kind === "clips") {
        history.execute(new DeleteClipsCommand(ctx.clipIds));
        uiStore.setSelectedClipIds([]);
      } else if (ctx.kind === "tracks") {
        history.execute(new DeleteTrackCommand(ctx.trackIds[0]));
        uiStore.setSelectedTrackId(null);
        uiStore.setSelectedMixerTrackId(null);
      }
      // Phase 3: ctx.kind === "midi-notes"  → delete selected notes
      // Phase 2: ctx.kind === "insert-device" → remove device from track
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
      const sel = buildSelectionState(uiStore);
      const ctx = getActiveSelectionContext(sel);
      if (ctx.kind === "clips") {
        history.execute(new DuplicateClipsCommand(ctx.clipIds));
      }
      // Phase 3: ctx.kind === "midi-notes" → duplicate notes
      break;
    }

    case "edit:select-all": {
      const sel = buildSelectionState(uiStore);
      const ctx = getActiveSelectionContext(sel);
      if (ctx.kind === "midi-notes") {
        // Let the MIDI editor select all notes in the active clip
        midiEditorBridge.call("selectAll");
      } else {
        // Arrangement default: select all clips in the project
        const allIds = projectStore.project.tracks.flatMap((t) => t.clips.map((c) => c.id));
        uiStore.setSelectedClipIds(allIds);
      }
      break;
    }

    case "edit:deselect-all":
      uiStore.setSelectedClipIds([]);
      uiStore.setSelectedTrackId(null);
      break;

    case "edit:select-track-clips": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (track) uiStore.setSelectedClipIds(track.clips.map((c) => c.id));
      break;
    }

    case "edit:select-loop-range":
      break;

    // ── Clip actions ───────────────────────────────────────────────────────
    case "clip:split-at-playhead": {
      const { selectedClipIds } = uiStore;
      if (selectedClipIds.length === 0) break;
      const t = activeAudioEngine.projectTime;
      selectedClipIds.forEach((id) => history.execute(new SplitClipCommand(id, t)));
      uiStore.setSelectedClipIds([]);
      break;
    }

    case "clip:trim-start-to-playhead":
    case "clip:trim-end-to-playhead":
    case "clip:crop-to-selection":
    case "clip:consolidate":
    case "clip:reverse":
      break;

    // ── Snap ──────────────────────────────────────────────────────────────
    case "timeline:toggle-snap":
      uiStore.toggleSnapToGrid();
      break;

    case "timeline:set-snap-bar":
    case "timeline:set-snap-whole":
    case "timeline:set-snap-beat":
    case "timeline:set-snap-eighth":
    case "timeline:set-snap-sixteenth":
    case "timeline:set-snap-thirty-second":
    case "timeline:set-snap-sixty-fourth":
    case "timeline:set-snap-quarter-triplet":
    case "timeline:set-snap-eighth-triplet":
    case "timeline:set-snap-sixteenth-triplet":
      if (!useUIStore.getState().snapToGrid) uiStore.toggleSnapToGrid();
      uiStore.setArrangementGridDivision(gridDivisionFromAction(actionId));
      break;

    case "timeline:set-snap-off":
      if (useUIStore.getState().snapToGrid) uiStore.toggleSnapToGrid();
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

    case "track:delete": {
      const { selectedTrackId } = uiStore;
      if (selectedTrackId) {
        history.execute(new DeleteTrackCommand(selectedTrackId));
        uiStore.setSelectedTrackId(null);
        uiStore.setSelectedMixerTrackId(null);
      }
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

    case "track:preview:stereo":
    case "track:preview:mono":
    case "track:preview:mid":
    case "track:preview:side": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (!track) break;
      const mode = actionId.slice("track:preview:".length) as TrackPreviewMode;
      history.execute(new SetTrackPreviewModeCommand(selectedTrackId, mode, track.monitor?.previewMode ?? "stereo"));
      break;
    }

    case "track:reset-fader": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (track && track.volume !== 1) history.execute(new SetTrackVolumeCommand(selectedTrackId, 1, track.volume));
      break;
    }

    case "track:reset-pan": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (track && track.pan !== 0) history.execute(new SetTrackPanCommand(selectedTrackId, 0, track.pan));
      break;
    }

    // stubs — not yet implemented
    case "track:add-insert":
    case "track:add-send":
    case "track:change-color":
    case "track:freeze":
    case "track:flatten":
    case "track:route-to":
    case "track:settings":
      break;

    case "track:show-add-dialog":
      void openAddTrackWindow();
      break;

    case "track:add-audio": {
      const tracks = projectStore.project.tracks;
      const newId = crypto.randomUUID();
      const newTrack: DawTrack = {
        id: newId,
        name: `Audio Track ${tracks.length + 1}`,
        type: "audio",
        color: getTrackColor(tracks.length),
        channelCount: 2,
        channelMode: "stereo",
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
        sends: [],
        inserts: [],
        output: "master",
        routing: { inputType: "system-audio", outputType: "master" },
        advanced: { latencyMs: 0, delayMs: 0, semitone: 0, phaseInvert: false, midSideMode: "off" },
        monitorMode: "off",
      };
      history.execute(new AddTrackCommand(newTrack));
      uiStore.setSelectedTrackId(newId);
      break;
    }

    case "track:add-midi": {
      const tracks = projectStore.project.tracks;
      const id = crypto.randomUUID();
      const newTrack: DawTrack = {
        id,
        name: `MIDI Track ${tracks.filter((t) => t.type === "midi").length + 1}`,
        type: "midi",
        color: getTrackColor(tracks.length),
        channelCount: 2,
        channelMode: "stereo",
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
        sends: [],
        inserts: [],
        routing: { inputType: "midi-device", outputType: "none" },
        advanced: { latencyMs: 0, delayMs: 0, semitone: 0, phaseInvert: false, midSideMode: "off" },
        monitorMode: "off",
      };
      history.execute(new AddTrackCommand(newTrack));
      uiStore.setSelectedTrackId(id);
      break;
    }

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

    case "track:add-plugin":
    case "track:add-master-bus":
      break;

    // ── Audio device actions ───────────────────────────────────────────────
    case "audio:refresh-devices":
      void audioDeviceService.refreshAudioDevices();
      break;

    case "audio:enable-input":
      void audioDeviceService.requestAudioPermission();
      break;

    case "audio.runSelfTest":
    case "audio:run-self-test":
      void activeAudioEngine.runSelfTest().then((result) => {
        showToast(
          result.ok
            ? `Audio self-test OK: ${result.backend}`
            : `Audio self-test failed: ${result.error ?? result.backend}`,
          !result.ok,
        );
      });
      break;

    // ── MIDI device actions ────────────────────────────────────────────────
    case "midi:enable":
      void midiDeviceService.requestMidiAccess();
      break;

    case "midi:refresh-devices":
      midiDeviceService.refreshMidiDevices();
      break;

    // ── MIDI editor actions (routed through bridge to active panel) ─────────
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

    // ── Project ────────────────────────────────────────────────────────────
    case "project:new":
      void guardUnsavedProject("new").then((ok) => {
        if (!ok) return;
        void openProjectWizardWindow();
      });
      break;

    case "project:rename": {
      const newName = window.prompt("Rename project:", projectStore.project.name)?.trim();
      if (newName && newName !== projectStore.project.name) {
        projectStore.setProjectName(newName);
      }
      break;
    }

    case "project:save":
      void saveCurrentProjectAndRemember();
      break;

    case "project:save-as":
    case "project:save-copy":
      useUIStore.getState().setSaveStatus("saving");
      void platform.projectStorage
        .saveProject(useProjectStore.getState().project, { saveAs: true })
        .then((result) => {
          if (result) {
            useUIStore.getState().setSaveStatus("saved");
            rememberSavedProject(useProjectStore.getState().project, result);
          } else {
            useUIStore.getState().setSaveStatus("unsaved");
          }
        })
        .catch((e) => {
          console.warn("[ActionRunner] save-as:", e);
          useUIStore.getState().setSaveStatus("error");
        });
      break;

    case "project:revert":
      projectStore.loadLocal();
      history.clear();
      break;

    case "project:close":
      void guardUnsavedProject("close").then((ok) => {
        if (!ok) return;
        platform.folderProject.setProjectRoot(null);
        useProjectStore.setState({
          project: {
            id: crypto.randomUUID(),
            name: "Untitled Project",
            version: 1,
            sampleRate: 48000,
            bpm: 120,
            timeSignature: { numerator: 4, denominator: 4 },
            tracks: [],
            files: [],
          },
        });
        history.clear();
        uiStore.setSelectedClipIds([]);
        uiStore.setSelectedTrackId(null);
        uiStore.setSelectedBrowserFileId(null);
        uiStore.setSaveStatus("saved");
      });
      break;

    case "project:recent-clear":
      useRecentProjectsStore.getState().clearRecentProjects();
      break;

    case "project:open":
      void guardUnsavedProject("open").then((ok) => {
        if (!ok) return;
        return platform.projectStorage.openProject().then((p) => {
          if (p) {
            void loadOpenedProject(p);
          }
        });
      }).catch((e) => console.warn("[ActionRunner] open project:", e));
      break;

    case "project:new-from-template":
    case "project:snapshot":
    case "project:collect-all-and-save":
    case "project:clean-unused-files":
    case "project:statistics":
      break;

    case "project:settings":
    case "project:tempo-settings":
    case "project:time-signature": {
      void openSettingsWindow("project");
      break;
    }

    case "project:set-sample-rate-44100":
    case "project:set-sample-rate-48000":
    case "project:set-sample-rate-88200":
    case "project:set-sample-rate-96000":
    case "project:set-sample-rate-192000":
    case "project:set-bit-depth-16":
    case "project:set-bit-depth-24":
    case "project:set-bit-depth-32float":
      break;

    // ── Markers (not yet implemented) ──────────────────────────────────────
    case "marker:add":
    case "marker:next":
    case "marker:previous":
    case "marker:manager":
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

    case "file:import-midi":
    case "file:import-stems":
    case "file:import-folder":
    case "file:import-session":
    case "file:export-audio":
    case "file:export-stems":
    case "file:export-loop":
    case "file:export-project-archive":
    case "file:bounce-selection":
    case "file:reveal-in-folder":
      break;

    // ── Audio processing (not yet implemented) ─────────────────────────────
    case "audio:settings": {
      void openSettingsWindow("audio");
      break;
    }

    case "audio:normalize-clip":
    case "audio:reverse-clip":
    case "audio:add-fade-in":
    case "audio:add-fade-out":
    case "audio:create-crossfade":
    case "audio:clip-gain":
    case "audio:bounce-in-place":
    case "audio:render-selection":
    case "audio:freeze-selected-tracks":
    case "audio:set-device-default":
    case "audio:set-device-web-audio":
    case "audio:set-buffer-64":
    case "audio:set-buffer-128":
    case "audio:set-buffer-256":
    case "audio:set-buffer-512":
    case "audio:set-buffer-1024":
      break;

    // ── Cloud (not yet implemented) ────────────────────────────────────────
    case "cloud:sync":
    case "cloud:share-project":
    case "cloud:versions":
      break;

    // ── Panel toggles ─────────────────────────────────────────────────────
    case "panel:toggle-browser":
    case "window.show_browser":
      uiStore.togglePanel("browser");
      break;

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

    // stubs — panels not yet implemented
    case "panel:toggle-device-panel":
    case "panel:toggle-midi-editor":
      break;

    case "panel:toggle-automation":
      // Toggle automation lanes visibility for selected track
      runAction("automation:toggle-lanes");
      break;

    // ── Automation ────────────────────────────────────────────────────────────
    case "automation:add-volume-lane": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const target = createTrackVolumeTarget(selectedTrackId);
      const existing = projectStore.project.tracks
        .find((t) => t.id === selectedTrackId)
        ?.automationLanes?.some((l) => l.target.id === target.id);
      if (!existing) projectStore.addAutomationLane(selectedTrackId, target);
      break;
    }

    case "automation:add-pan-lane": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const target = createTrackPanTarget(selectedTrackId);
      const existing = projectStore.project.tracks
        .find((t) => t.id === selectedTrackId)
        ?.automationLanes?.some((l) => l.target.id === target.id);
      if (!existing) projectStore.addAutomationLane(selectedTrackId, target);
      break;
    }

    case "automation:toggle-lanes": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      if (!track) break;
      const lanes = track.automationLanes ?? [];
      const anyVisible = lanes.some((l) => l.visible);
      for (const lane of lanes) {
        if (anyVisible ? lane.visible : !lane.visible) {
          projectStore.toggleAutomationLaneVisible(selectedTrackId, lane.id);
        }
      }
      break;
    }

    case "automation:clear-lane": {
      const { selectedTrackId } = uiStore;
      if (!selectedTrackId) break;
      const track = projectStore.project.tracks.find((t) => t.id === selectedTrackId);
      const lane = track?.automationLanes?.[0];
      if (lane) projectStore.clearAutomationLane(selectedTrackId, lane.id);
      break;
    }

    case "automation:delete-selected-points":
    case "automation:set-curve-linear":
    case "automation:set-curve-hold":
      // Handled inside AutomationLaneView context menus — no global selection yet.
      break;

    case "automation:set-mode-off":
    case "automation:set-mode-read":
    case "automation:set-mode-touch":
    case "automation:set-mode-latch":
    case "automation:set-mode-write":
      // Automation mode — Read is default; Touch/Latch/Write are disabled in menu.
      break;

    // ── Panel dock positions ───────────────────────────────────────────────
    case "panel:browser-dock-left":
      uiStore.setPanelLayout("browser", { dock: "left" });
      break;
    case "panel:browser-dock-right":
      uiStore.setPanelLayout("browser", { dock: "right" });
      break;
    case "panel:browser-dock-bottom":
      uiStore.setPanelLayout("browser", { dock: "bottom" });
      break;
    case "panel:browser-float":
      uiStore.setPanelLayout("browser", { dock: "float" });
      break;

    case "panel:inspector-dock-left":
      uiStore.setPanelLayout("inspector", { dock: "left" });
      break;
    case "panel:inspector-dock-right":
      uiStore.setPanelLayout("inspector", { dock: "right" });
      break;
    case "panel:inspector-dock-bottom":
      uiStore.setPanelLayout("inspector", { dock: "bottom" });
      break;
    case "panel:inspector-float":
      uiStore.setPanelLayout("inspector", { dock: "float" });
      break;

    case "panel:mixer-dock-left":
      uiStore.setPanelLayout("mixer", { dock: "left" });
      break;
    case "panel:mixer-dock-right":
      uiStore.setPanelLayout("mixer", { dock: "right" });
      break;
    case "panel:mixer-dock-bottom":
      uiStore.setPanelLayout("mixer", { dock: "bottom" });
      break;
    case "panel:mixer-float":
      uiStore.setPanelLayout("mixer", { dock: "float" });
      break;

    case "floatingwindow:mixer":
      void (async () => {
        useProjectStore.getState().saveLocal();
        const opened = await window.dawElectron?.windows?.openExternal({
          id: "mixer",
          contentType: "mixer",
          title: "Mixer - Futureboard",
          width: 1180,
          height: 420,
          minWidth: 760,
          minHeight: 320,
          alwaysOnTop: false,
          frame: true,
          transparent: false,
          resizable: true,
        });
        if (!opened) {
          uiStore.setPanelLayout("mixer", { dock: "float" });
          showToast("External mixer unavailable; opened internal mixer.", true);
          return;
        }
        uiStore.setPanelLayout("mixer", { visible: false });
      })();
      break;

    // ── Zoom ──────────────────────────────────────────────────────────────
    case "view:zoom-in":
      uiStore.setPixelsPerSecond(Math.min(800, uiStore.pixelsPerSecond * 1.33));
      break;

    case "view:zoom-out":
      uiStore.setPixelsPerSecond(Math.max(10, uiStore.pixelsPerSecond * 0.75));
      break;

    case "view:reset-zoom":
      uiStore.setPixelsPerSecond(100);
      break;

    // ── Workspace layouts ──────────────────────────────────────────────────
    case "layout:default":
    case "layout:reset-current":
      uiStore.applyWorkspaceLayout("Default");
      break;
    case "layout:editing":
      uiStore.applyWorkspaceLayout("Editing");
      break;
    case "layout:mixing":
      uiStore.applyWorkspaceLayout("Mixing");
      break;
    case "layout:sound-design":
      uiStore.applyWorkspaceLayout("Sound Design");
      break;
    case "layout:minimal":
      uiStore.applyWorkspaceLayout("Minimal");
      break;
    case "layout:laptop":
      uiStore.applyWorkspaceLayout("Laptop");
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
      void guardUnsavedProject("close").then((ok) => {
        if (ok) platform.window.close();
      });
      break;

    case "window:toggle-fullscreen":
      if (document.fullscreenElement) {
        void document.exitFullscreen().catch(() => {});
      } else {
        void document.documentElement.requestFullscreen().catch(() => {});
      }
      break;

    case "window:toggle-always-on-top":
      break;

    // ── App ────────────────────────────────────────────────────────────────
    case "app:quit":
    case "app:request-close":
      void guardUnsavedProject("close").then((ok) => {
        if (ok) (platform.window.forceClose ?? platform.window.close)();
      });
      break;

    case "app:reload":
    case "app:force-reload":
      window.location.reload();
      break;

    case "app:preferences": {
      void openSettingsWindow();
      break;
    }
    case "app:check-for-updates":
    case "app:about":
      break;

    // ── Tools stubs ────────────────────────────────────────────────────────
    case "tools:audio-analyzer":
    case "tools:loudness-meter":
    case "tools:spectrum-analyzer":
    case "tools:phase-meter":
    case "tools:media-pool":
    case "tools:sample-browser":
    case "tools:loop-browser":
    case "tools:marker-list":
    case "tools:developer-tools":
    case "tools:performance-monitor":
      break;

    // ── Help stubs ─────────────────────────────────────────────────────────
    case "help:keyboard-shortcuts": {
      void openSettingsWindow("shortcuts");
      break;
    }
    case "help:quick-start":
    case "help:documentation":
    case "help:release-notes":
    case "help:roadmap":
    case "help:github":
    case "help:report-issue":
    case "help:request-feature":
    case "help:community":
    case "help:diagnostics":
    case "help:copy-system-info":
      break;

    // ── Plugin stubs ───────────────────────────────────────────────────────
    case "plugins:scan":
      break;

    case "plugins:manager":
      void openPluginManagerWindow();
      break;

    case "plugins:format-vst3":
    case "plugins:format-clap":
    case "plugins:format-daux":
      break;

    case "noop":
      break;

    default:
      // project:open-file:<path> — sent by Electron when the app is opened with a .mochiproj file
      if (actionId.startsWith("project:open-file:")) {
        const filePath = actionId.slice("project:open-file:".length);
        if (filePath && platform.folderProject.isSupported) {
          void guardUnsavedProject("open").then((ok) => {
            if (!ok) return;
            return openProjectFromPath(filePath);
          }).catch((e: unknown) => console.warn("[ActionRunner] open-file:", e));
        }
        break;
      }
      // file:reveal-in-folder:<path>
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

function gridDivisionFromAction(actionId: string): SnapDivision {
  switch (actionId) {
    case "timeline:set-snap-auto": return "auto";
    case "timeline:set-snap-bar": return "1bar";
    case "timeline:set-snap-whole": return "1/1";
    case "timeline:set-snap-beat": return "1/4";
    case "timeline:set-snap-eighth": return "1/8";
    case "timeline:set-snap-sixteenth": return "1/16";
    case "timeline:set-snap-thirty-second": return "1/32";
    case "timeline:set-snap-sixty-fourth": return "1/64";
    case "timeline:set-snap-quarter-triplet": return "1/4T";
    case "timeline:set-snap-eighth-triplet": return "1/8T";
    case "timeline:set-snap-sixteenth-triplet": return "1/16T";
    default: return "auto";
  }
}
