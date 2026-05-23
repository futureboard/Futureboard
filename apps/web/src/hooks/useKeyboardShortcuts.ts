import { useEffect } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useTransportStore } from "../store/transportStore";
import { useHistoryStore } from "../store/historyStore";
import { activeAudioEngine } from "../engine/activeAudioEngine";
import { beatsPerBar, secondsPerBeat } from "../utils/musicalTime";
import { DeleteClipsCommand, DeleteTrackCommand, DuplicateClipsCommand, SplitClipCommand } from "../commands";
import { useMetronomeStore } from "../store/metronomeStore";
import { runAction } from "../menu/actionRunner";

/** Returns true when the event target is a text input — most shortcuts should be suppressed. */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || !(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (target.isContentEditable) return true;
  const role = target.getAttribute("role");
  if (role === "textbox" || role === "combobox" || role === "searchbox") return true;
  return false;
}

export function useKeyboardShortcuts() {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      const shift = e.shiftKey;
      const inEditable = isEditableTarget(e.target);

      // ── Global shortcuts — always fire regardless of focus ───────────────
      // These must preventDefault to block browser default file dialogs.
      if (ctrl && !shift && e.code === "KeyS") {
        e.preventDefault();
        runAction("project:save");
        return;
      }
      if (ctrl && shift && e.code === "KeyS") {
        e.preventDefault();
        runAction("project:save-as");
        return;
      }
      if (ctrl && !shift && e.code === "KeyO") {
        e.preventDefault();
        if (!inEditable) runAction("project:open");
        return;
      }
      if (ctrl && !shift && e.code === "KeyN") {
        e.preventDefault();
        if (!inEditable) runAction("project:new");
        return;
      }
      if (ctrl && !shift && e.code === "KeyI") {
        e.preventDefault();
        if (!inEditable) runAction("file:import-audio");
        return;
      }
      if (ctrl && !shift && e.code === "KeyR") {
        e.preventDefault();
        if (!inEditable) runAction("audio:render-selection");
        return;
      }
      if (ctrl && !shift && e.code === "Comma") {
        e.preventDefault();
        if (!inEditable) runAction("app:preferences");
        return;
      }

      // ── Block non-global shortcuts while typing ──────────────────────────
      if (inEditable) return;

      const {
        pixelsPerSecond,
        setPixelsPerSecond,
        selectedClipIds,
        setSelectedClipIds,
        toggleSnapToGrid,
        togglePanel,
        toggleLoop,
        toggleCommandPalette,
      } = useUIStore.getState();

      const { project } = useProjectStore.getState();
      const { isPlaying, setIsPlaying } = useTransportStore.getState();
      const history = useHistoryStore.getState();
      const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };
      const spb = secondsPerBeat(project.bpm);
      const bpb = beatsPerBar(timeSig);

      switch (e.code) {
        // ── Transport ──────────────────────────────────────────────────────
        case "Space": {
          e.preventDefault();
          if (isPlaying) {
            activeAudioEngine.pause();
            setIsPlaying(false);
          } else {
            void activeAudioEngine.play().then(() => setIsPlaying(true));
          }
          break;
        }
        case "Enter": {
          e.preventDefault();
          activeAudioEngine.stop();
          setIsPlaying(false);
          break;
        }
        case "Home": {
          e.preventDefault();
          activeAudioEngine.seekSeconds(0);
          break;
        }

        // ── Playhead nudge: ← / → by 1 beat; Shift+← / Shift+→ by 1 bar ──
        case "ArrowLeft": {
          if (ctrl) break;
          e.preventDefault();
          const nudgeL = shift ? spb * bpb : spb;
          activeAudioEngine.seekSeconds(Math.max(0, activeAudioEngine.projectTime - nudgeL));
          break;
        }
        case "ArrowRight": {
          if (ctrl) break;
          e.preventDefault();
          const nudgeR = shift ? spb * bpb : spb;
          activeAudioEngine.seekSeconds(activeAudioEngine.projectTime + nudgeR);
          break;
        }

        // ── Zoom ──────────────────────────────────────────────────────────
        case "Equal":
        case "NumpadAdd": {
          if (ctrl) break;
          e.preventDefault();
          setPixelsPerSecond(Math.min(800, pixelsPerSecond * 1.33));
          break;
        }
        case "Minus":
        case "NumpadSubtract": {
          if (ctrl) break;
          e.preventDefault();
          setPixelsPerSecond(Math.max(10, pixelsPerSecond * 0.75));
          break;
        }
        case "Digit0":
        case "Numpad0": {
          if (!ctrl) break;
          e.preventDefault();
          setPixelsPerSecond(100);
          break;
        }

        // ── Edit ──────────────────────────────────────────────────────────
        case "Delete":
        case "Backspace": {
          const { focusedPanel } = useUIStore.getState();
          if (focusedPanel === "timeline" && selectedClipIds.length > 0) {
            history.execute(new DeleteClipsCommand(selectedClipIds));
            setSelectedClipIds([]);
          } else if (focusedPanel === "timeline") {
            const { selectedTrackId } = useUIStore.getState();
            if (selectedTrackId) {
              history.execute(new DeleteTrackCommand(selectedTrackId));
              useUIStore.getState().setSelectedTrackId(null);
              useUIStore.getState().setSelectedMixerTrackId(null);
            }
          }
          break;
        }
        case "Escape": {
          setSelectedClipIds([]);
          useUIStore.getState().setSelectedTrackId(null);
          useUIStore.getState().setSelectedMixerTrackId(null);
          break;
        }

        // ── Edit via runAction ─────────────────────────────────────────────
        case "KeyZ": {
          if (!ctrl) break;
          e.preventDefault();
          runAction(shift ? "edit:redo" : "edit:undo");
          break;
        }
        case "KeyY": {
          if (!ctrl) break;
          e.preventDefault();
          runAction("edit:redo");
          break;
        }
        case "KeyD": {
          if (!ctrl) break;
          e.preventDefault();
          const { selectedClipIds: ids } = useUIStore.getState();
          if (ids.length > 0) {
            history.execute(new DuplicateClipsCommand(ids));
          }
          break;
        }

        // ── Clip split ────────────────────────────────────────────────────
        case "KeyS": {
          // Ctrl+S is handled above. Plain S splits at playhead.
          if (ctrl) break;
          e.preventDefault();
          const ids = useUIStore.getState().selectedClipIds;
          if (ids.length > 0) {
            const t = activeAudioEngine.projectTime;
            ids.forEach((id) => history.execute(new SplitClipCommand(id, t)));
            setSelectedClipIds([]);
          }
          break;
        }
        case "KeyX": {
          if (ctrl) break;
          e.preventDefault();
          const ids = useUIStore.getState().selectedClipIds;
          if (ids.length > 0) {
            const t = activeAudioEngine.projectTime;
            ids.forEach((id) => history.execute(new SplitClipCommand(id, t)));
            setSelectedClipIds([]);
          }
          break;
        }

        // ── Arrangement tools ─────────────────────────────────────────────
        case "KeyV": {
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("pointer"); }
          break;
        }
        case "KeyP": {
          if (ctrl) break;
          e.preventDefault();
          useUIStore.getState().setCurrentTool("pen");
          break;
        }
        case "KeyC": {
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("cut"); }
          break;
        }
        case "KeyG": {
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("glue"); }
          break;
        }
        case "KeyT": {
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("time"); }
          break;
        }
        case "KeyA": {
          if (ctrl) { e.preventDefault(); runAction("edit:select-all"); }
          else { e.preventDefault(); useUIStore.getState().setCurrentTool("automation"); }
          break;
        }

        // ── View toggles ──────────────────────────────────────────────────
        case "KeyK": {
          if (ctrl) {
            e.preventDefault();
            toggleCommandPalette();
          } else {
            useMetronomeStore.getState().toggle();
          }
          break;
        }
        case "KeyM": {
          if (!ctrl) togglePanel("mixer");
          break;
        }
        case "KeyI": {
          // Ctrl+I handled above; plain I toggles inspector
          if (!ctrl) togglePanel("inspector");
          break;
        }
        case "KeyN": {
          // Ctrl+N handled above; plain N toggles snap
          if (!ctrl) toggleSnapToGrid();
          break;
        }
        case "KeyL": {
          if (!ctrl) toggleLoop();
          break;
        }
        case "KeyB": {
          if (!ctrl) togglePanel("browser");
          break;
        }

        // ── Panel shortcuts ───────────────────────────────────────────────
        case "Digit1": {
          if (ctrl) { e.preventDefault(); togglePanel("browser"); }
          break;
        }
        case "Digit2": {
          if (ctrl) { e.preventDefault(); togglePanel("inspector"); }
          break;
        }
        case "Digit3": {
          if (ctrl) { e.preventDefault(); togglePanel("mixer"); }
          break;
        }

        // ── Navigation ────────────────────────────────────────────────────
        case "End": {
          e.preventDefault();
          const { tracks } = useProjectStore.getState().project;
          const end = tracks.reduce((max, tr) =>
            tr.clips.reduce((m, c) => Math.max(m, c.startTime + c.duration), max), 0);
          activeAudioEngine.seekSeconds(end);
          break;
        }
      }
    };

    const onMainShortcut = (e: Event) => {
      const action = (e as CustomEvent<string>).detail;
      if (action === "audio:render-selection" && !isEditableTarget(document.activeElement)) {
        runAction(action);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("futureboard:main-shortcut", onMainShortcut);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("futureboard:main-shortcut", onMainShortcut);
    };
  }, []); // reads state from Zustand getState() — no deps needed
}
