import { useEffect } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useTransportStore } from "../store/transportStore";
import { transport } from "../engine/Transport";
import { clipScheduler } from "../engine/ClipScheduler";
import { beatsPerBar, secondsPerBeat } from "../utils/musicalTime";

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
        selectedClipId,
        setSelectedClipId,
        toggleSnapToGrid,
        toggleMixer,
        toggleInspector,
        toggleLoop,
      } = useUIStore.getState();

      const { project, removeClip, saveLocal } = useProjectStore.getState();
      const { isPlaying, setIsPlaying } = useTransportStore.getState();
      const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };
      const spb = secondsPerBeat(project.bpm);
      const bpb = beatsPerBar(timeSig);

      switch (e.code) {
        // ── Transport ──────────────────────────────────────────────────────
        case "Space": {
          e.preventDefault();
          if (isPlaying) {
            transport.pause();
            clipScheduler.cancelAll();
            setIsPlaying(false);
          } else {
            void transport.play(() => {
              clipScheduler.schedule(project.tracks);
              setIsPlaying(true);
            });
          }
          break;
        }
        case "Enter": {
          e.preventDefault();
          transport.stop(() => {
            clipScheduler.cancelAll();
            setIsPlaying(false);
          });
          break;
        }
        case "Home": {
          e.preventDefault();
          transport.seek(0);
          if (isPlaying) {
            clipScheduler.cancelAll();
            clipScheduler.schedule(project.tracks);
          }
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
          if (selectedClipId) {
            removeClip(selectedClipId);
            setSelectedClipId(null);
          }
          break;
        }
        case "Escape": {
          setSelectedClipId(null);
          break;
        }
        case "KeyS": {
          if (!ctrl) break;
          e.preventDefault();
          saveLocal();
          break;
        }
        case "KeyZ": {
          if (!ctrl) break;
          e.preventDefault();
          // Undo placeholder — prevents browser back navigation
          break;
        }
        case "KeyY": {
          if (!ctrl) break;
          e.preventDefault();
          // Redo placeholder
          break;
        }

        // ── View toggles ───────────────────────────────────────────────────
        case "KeyM": {
          if (!ctrl) toggleMixer();
          break;
        }
        case "KeyI": {
          if (!ctrl) toggleInspector();
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
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []); // reads state from Zustand getState() — no deps needed
}
