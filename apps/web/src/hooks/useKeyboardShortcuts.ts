import { useEffect } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useTransportStore } from "../store/transportStore";
import { useMetronomeStore } from "../store/metronomeStore";
import { useHistoryStore } from "../store/historyStore";
import { transport } from "../engine/Transport";
import { beatsPerBar, secondsPerBeat } from "../utils/musicalTime";
import { DeleteClipsCommand, DeleteTrackCommand, DuplicateClipsCommand, SplitClipCommand } from "../commands";

function isTyping(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement;
  return (
    t.tagName === "INPUT" ||
    t.tagName === "TEXTAREA" ||
    t.tagName === "SELECT" ||
    t.isContentEditable
  );
}

export function useKeyboardShortcuts() {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isTyping(e)) return;

      const ctrl = e.ctrlKey || e.metaKey;
      const shift = e.shiftKey;

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

      const { project, saveLocal } = useProjectStore.getState();
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
            transport.pause();
            setIsPlaying(false);
          } else {
            void transport.play().then(() => setIsPlaying(true));
          }
          break;
        }
        case "Enter": {
          e.preventDefault();
          transport.stop();
          setIsPlaying(false);
          break;
        }
        case "Home": {
          e.preventDefault();
          transport.seek(0);
          break;
        }

        // ── Playhead nudge ─────────────────────────────────────────────────
        // ← / → by 1 beat; Shift+← / Shift+→ by 1 bar
        case "ArrowLeft": {
          if (ctrl) break;
          e.preventDefault();
          const nudgeL = shift ? spb * bpb : spb;
          transport.seek(Math.max(0, transport.projectTime - nudgeL));
          break;
        }
        case "ArrowRight": {
          if (ctrl) break;
          e.preventDefault();
          const nudgeR = shift ? spb * bpb : spb;
          transport.seek(transport.projectTime + nudgeR);
          break;
        }

        // ── Zoom ───────────────────────────────────────────────────────────
        case "Equal":       // +/=
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

        // ── Edit ───────────────────────────────────────────────────────────
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
        case "KeyS": {
          if (ctrl) {
            e.preventDefault();
            saveLocal();
          } else {
            e.preventDefault();
            const ids = useUIStore.getState().selectedClipIds;
            if (ids.length > 0) {
              const t = transport.projectTime;
              ids.forEach((id) => history.execute(new SplitClipCommand(id, t)));
              setSelectedClipIds([]);
            }
          }
          break;
        }
        case "KeyX": {
          if (!ctrl) {
            e.preventDefault();
            const ids = useUIStore.getState().selectedClipIds;
            if (ids.length > 0) {
              const t = transport.projectTime;
              ids.forEach((id) => history.execute(new SplitClipCommand(id, t)));
              setSelectedClipIds([]);
            }
          }
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
        case "KeyZ": {
          if (!ctrl) break;
          e.preventDefault();
          if (shift) history.redo();
          else history.undo();
          break;
        }
        case "KeyY": {
          if (!ctrl) break;
          e.preventDefault();
          history.redo();
          break;
        }

        // ── Arrangement tools ──────────────────────────────────────────────
        case "KeyV": {
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("pointer"); }
          break;
        }
        case "KeyP": {
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("pen"); }
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
          if (!ctrl) { e.preventDefault(); useUIStore.getState().setCurrentTool("automation"); }
          break;
        }

        // ── View toggles ───────────────────────────────────────────────────
        case "KeyK": {
          if (ctrl) {
            e.preventDefault();
            toggleCommandPalette();
          } else {
            const { toggle: toggleMetronome } = useMetronomeStore.getState();
            toggleMetronome();
          }
          break;
        }
        case "KeyM": {
          if (!ctrl) togglePanel("mixer");
          break;
        }
        case "KeyI": {
          if (!ctrl) togglePanel("inspector");
          break;
        }
        case "KeyN": {
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

        // ── Panel shortcuts ────────────────────────────────────────────────
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
          transport.seek(end);
          break;
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []); // reads state from Zustand getState() — no deps needed
}
