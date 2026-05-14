import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ChevronDown, Copy, Magnet, MousePointer2, Pencil, Trash2 } from "lucide-react";
import type { DawClip, DawTrack, MidiNote } from "../types/daw";
import { useProjectStore } from "../store/projectStore";
import { useHistoryStore } from "../store/historyStore";
import { AddMidiNotesCommand, RemoveMidiNotesCommand, UpdateMidiNotesCommand } from "../commands";

// ── Layout constants (CSS pixels — never scale by DPR for DOM) ───────────────
const ROW_H     = 14;    // px per semitone
const PITCH_CNT = 128;
const TOTAL_H   = PITCH_CNT * ROW_H; // 1792 CSS px
const VEL_H     = 72;
const MIN_DUR   = 0.03;  // seconds

// ── Note helpers ──────────────────────────────────────────────────────────────
const NOTE_NAMES = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];
const BLACK_SET  = new Set([1, 3, 6, 8, 10]);
function isBlack(pitch: number) { return BLACK_SET.has(pitch % 12); }
function noteName(pitch: number) { return NOTE_NAMES[pitch % 12] + (Math.floor(pitch / 12) - 1); }

// ── Grid resolution ───────────────────────────────────────────────────────────
// Values = BEATS PER GRID STEP (musical note values):
//   "1/4"  = quarter note  = 1 beat
//   "1/16" = sixteenth     = 0.25 beats
type GridRes = "1" | "1/2" | "1/4" | "1/8" | "1/16" | "1/32";
type EditorTool = "draw" | "select";

const GRID_FRACS: Record<GridRes, number> = {
  "1":    4,      // whole note  (4 beats)
  "1/2":  2,      // half note   (2 beats)
  "1/4":  1,      // quarter     (1 beat)
  "1/8":  0.5,    // eighth      (half beat)
  "1/16": 0.25,   // sixteenth   (quarter beat)
  "1/32": 0.125,  // thirty-second (eighth beat)
};
const GRID_OPTIONS: GridRes[] = ["1", "1/2", "1/4", "1/8", "1/16", "1/32"];

// ── Snap helpers ──────────────────────────────────────────────────────────────
function snapFloor(sec: number, spb: number, beatsPerStep: number): number {
  const step = spb * beatsPerStep;
  return Math.floor(sec / step) * step;
}
function snapRound(sec: number, spb: number, beatsPerStep: number): number {
  const step = spb * beatsPerStep;
  return Math.round(sec / step) * step;
}

// ── Canvas grid background ────────────────────────────────────────────────────
// DPR is applied ONLY here (canvas draw calls); DOM note positions use CSS px.
function drawGrid(
  canvas: HTMLCanvasElement,
  gridW: number,
  pps: number,
  spb: number,
  beatsPerStep: number,
  beatsPerBar: number,
) {
  const dpr = window.devicePixelRatio ?? 1;
  canvas.width        = Math.ceil(gridW * dpr);
  canvas.height       = TOTAL_H * dpr;
  canvas.style.width  = `${gridW}px`;
  canvas.style.height = `${TOTAL_H}px`;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.scale(dpr, dpr);

  // Row backgrounds (iterate low→high pitch, draw top→bottom)
  for (let p = 0; p < PITCH_CNT; p++) {
    const y = (PITCH_CNT - 1 - p) * ROW_H;
    ctx.fillStyle = isBlack(p) ? "rgba(0,0,0,0.28)" : "rgba(255,255,255,0.018)";
    ctx.fillRect(0, y, gridW, ROW_H);
    // horizontal divider — stronger at C
    ctx.fillStyle = p % 12 === 0 ? "rgba(255,255,255,0.10)" : "rgba(255,255,255,0.04)";
    ctx.fillRect(0, y, gridW, 1);
  }

  // Vertical time grid lines
  const step    = spb * beatsPerStep;   // seconds per subdivision
  const barLen  = spb * beatsPerBar;    // seconds per bar
  const eps     = step * 0.02;          // floating-point tolerance

  let t = 0;
  while (t * pps <= gridW + 1) {
    const x = t * pps;
    // Use modulo on t to detect bar/beat boundaries safely
    const tModBar  = ((t % barLen)  + barLen)  % barLen;
    const tModBeat = ((t % spb)     + spb)     % spb;
    const isBar    = tModBar  < eps || tModBar  > barLen - eps;
    const isBeat   = tModBeat < eps || tModBeat > spb    - eps;

    ctx.fillStyle = isBar
      ? "rgba(255,255,255,0.22)"
      : isBeat
        ? "rgba(255,255,255,0.11)"
        : "rgba(255,255,255,0.04)";
    ctx.fillRect(Math.round(x), 0, 1, TOTAL_H);
    t += step;
    if (t > 3600) break; // safety cap
  }
}

// ── Drag state ────────────────────────────────────────────────────────────────
type DragState =
  | { type: "idle" }
  | { type: "move";   ids: string[]; prevNotes: MidiNote[]; startX: number; startY: number }
  | { type: "resize"; id: string;   prevNote: MidiNote;    startX: number }
  | { type: "create"; id: string;   startTime: number;     startX: number; defaultDur: number };

// ── Main component ────────────────────────────────────────────────────────────
export function MidiEditorPanel({
  clip,
  track,
}: {
  clip: DawClip;
  track: DawTrack | null | undefined;
}) {
  const { bpm, timeSignature } = useProjectStore((s) => s.project);
  const spb         = 60 / bpm;                    // seconds per beat
  const beatsPerBar = timeSignature?.numerator ?? 4;

  const [ppb,      setPpb]      = useState(80);          // pixels per beat
  const [gridRes,  setGridRes]  = useState<GridRes>("1/16");
  const [snapOn,   setSnapOn]   = useState(true);
  const [tool,     setTool]     = useState<EditorTool>("draw");
  const [selIds,   setSelIds]   = useState<Set<string>>(new Set());
  const [gridOpen, setGridOpen] = useState(false);

  // pps = pixels per second (derived)
  const pps = ppb / spb;

  const notes: MidiNote[] = clip.notes ?? [];
  const trackColor = track?.color ?? "#a99cff";

  // ── DOM refs ──────────────────────────────────────────────────────────────
  const mainRef   = useRef<HTMLDivElement>(null);   // scroll container
  const pianoRef  = useRef<HTMLDivElement>(null);
  const velRef    = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragRef   = useRef<DragState>({ type: "idle" });

  // ── Mutable-value refs (prevent stale closures in window event handlers) ──
  // Pattern: assign on every render so handlers always read current values.
  const ppsRef         = useRef(pps);     ppsRef.current     = pps;
  const spbRef         = useRef(spb);     spbRef.current     = spb;
  const snapOnRef      = useRef(snapOn);  snapOnRef.current  = snapOn;
  const gridResRef     = useRef(gridRes); gridResRef.current = gridRes;
  const notesRef       = useRef(notes);   notesRef.current   = notes;
  const selIdsRef      = useRef(selIds);  selIdsRef.current  = selIds;

  // Live note state during a drag (null = render from store)
  const [liveNotes, setLiveNotes] = useState<MidiNote[] | null>(null);
  const liveNotesRef = useRef<MidiNote[] | null>(null);
  liveNotesRef.current = liveNotes;

  const displayNotes = liveNotes ?? notes;
  const gridW = Math.max(900, clip.duration * pps + 200);

  // ── Coordinate helpers ────────────────────────────────────────────────────
  // clientToGrid: viewport coords → grid content coords.
  // Uses the SCROLL CONTAINER's bounding rect (fixed in viewport) + scroll offsets.
  // Do NOT use the inner content div — its rect already bakes in scroll, causing double-counting.
  function clientToGrid(clientX: number, clientY: number) {
    const el = mainRef.current;
    if (!el) return { x: 0, y: 0 };
    const rect = el.getBoundingClientRect(); // scroll container, not inner div
    return {
      x: clientX - rect.left + el.scrollLeft,
      y: clientY - rect.top  + el.scrollTop,
    };
  }

  // All note position/size math goes through these helpers.
  // DOM positions are in CSS pixels (no DPR scaling).
  function xToTime(x: number):          number { return Math.max(0, x / ppsRef.current); }
  function timeToX(t: number):          number { return t * pps; }
  function yToPitch(y: number):         number {
    return Math.max(0, Math.min(PITCH_CNT - 1, PITCH_CNT - 1 - Math.floor(y / ROW_H)));
  }
  function pitchToY(pitch: number):     number { return (PITCH_CNT - 1 - pitch) * ROW_H; }
  function durationToWidth(d: number):  number { return d * pps; }

  function gridStepSec(): number { return spb * GRID_FRACS[gridRes]; }
  function doSnap(t: number):     number { return snapOn ? snapFloor(t, spb, GRID_FRACS[gridRes]) : t; }

  // ── Initial scroll: center on C4 (pitch 60) ──────────────────────────────
  useLayoutEffect(() => {
    const el = mainRef.current;
    if (!el) return;
    el.scrollTop = Math.max(0, pitchToY(60) - el.clientHeight / 2);
  // pitchToY is stable (depends only on constants), so empty deps is fine
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── Scroll sync ───────────────────────────────────────────────────────────
  const syncScroll = useCallback(() => {
    const main  = mainRef.current;
    const piano = pianoRef.current;
    const vel   = velRef.current;
    if (!main) return;
    if (piano) piano.scrollTop  = main.scrollTop;
    if (vel)   vel.scrollLeft   = main.scrollLeft;
  }, []);

  // ── Canvas redraw ─────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    drawGrid(canvas, gridW, pps, spb, GRID_FRACS[gridRes], beatsPerBar);
  }, [gridW, pps, spb, gridRes, beatsPerBar]);

  // ── Create note on background click (draw tool) ───────────────────────────
  function handleGridMouseDown(e: React.MouseEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.stopPropagation();

    if (tool === "select") {
      setSelIds(new Set());
      return;
    }

    // Use clientToGrid which correctly subtracts the scroll CONTAINER's rect
    const { x: gridX, y: gridY } = clientToGrid(e.clientX, e.clientY);

    const pitch     = yToPitch(gridY);
    const rawTime   = xToTime(gridX);
    const startTime = doSnap(rawTime);
    const defaultDur = gridStepSec();

    const newNote: MidiNote = {
      id: crypto.randomUUID(),
      pitch,
      start: startTime,
      duration: defaultDur,
      velocity: 100,
    };

    useHistoryStore.getState().execute(new AddMidiNotesCommand(clip.id, [newNote]));
    setSelIds(new Set([newNote.id]));
    dragRef.current = { type: "create", id: newNote.id, startTime, startX: e.clientX, defaultDur };
    setLiveNotes([...notesRef.current, newNote]);
  }

  // ── Note mousedown: select + start move/resize ────────────────────────────
  function handleNoteMouseDown(e: React.MouseEvent, note: MidiNote, resize = false) {
    if (e.button !== 0) return;
    e.stopPropagation();

    // Compute affected IDs BEFORE calling setSelIds (state update is async).
    // Use selIdsRef.current which is always synchronously up-to-date.
    let affectedIds: string[];
    if (resize) {
      affectedIds = [note.id];
    } else if (e.shiftKey) {
      // Toggle: new selection = old ± this note; drag only this note
      const next = new Set(selIdsRef.current);
      next.has(note.id) ? next.delete(note.id) : next.add(note.id);
      setSelIds(next);
      affectedIds = [note.id];
    } else if (selIdsRef.current.has(note.id)) {
      // Drag all currently selected notes
      affectedIds = [...selIdsRef.current];
    } else {
      setSelIds(new Set([note.id]));
      affectedIds = [note.id];
    }

    if (resize) {
      dragRef.current = { type: "resize", id: note.id, prevNote: { ...note }, startX: e.clientX };
    } else {
      const prevNotes = notesRef.current
        .filter((n) => affectedIds.includes(n.id))
        .map((n) => ({ ...n }));
      dragRef.current = { type: "move", ids: affectedIds, prevNotes, startX: e.clientX, startY: e.clientY };
    }
    setLiveNotes([...notesRef.current]);
  }

  // ── Global drag handlers ──────────────────────────────────────────────────
  // Deps: only clip.id — all mutable values read through refs to avoid
  // registering/unregistering listeners on every state change (which would
  // tear down active drags mid-gesture).
  useEffect(() => {
    function onMove(e: MouseEvent) {
      const drag = dragRef.current;
      if (drag.type === "idle") return;

      // Read current values via refs — no stale closures
      const curPps          = ppsRef.current;
      const curSpb          = spbRef.current;
      const curSnap         = snapOnRef.current;
      const curBeatsPerStep = GRID_FRACS[gridResRef.current];
      const curNotes        = notesRef.current;

      if (drag.type === "move") {
        const dtSec  = (e.clientX - drag.startX) / curPps;
        const dpitch = -Math.round((e.clientY - drag.startY) / ROW_H);

        setLiveNotes(curNotes.map((n) => {
          if (!drag.ids.includes(n.id)) return n;
          const prev = drag.prevNotes.find((p) => p.id === n.id)!;
          let newStart = Math.max(0, prev.start + dtSec);
          if (curSnap) newStart = snapFloor(newStart, curSpb, curBeatsPerStep);
          const newPitch = Math.max(0, Math.min(PITCH_CNT - 1, prev.pitch + dpitch));
          return { ...n, start: newStart, pitch: newPitch };
        }));
      }

      if (drag.type === "resize") {
        const dx = e.clientX - drag.startX;
        let newDur = Math.max(MIN_DUR, drag.prevNote.duration + dx / curPps);
        if (curSnap) {
          const snappedEnd = snapRound(drag.prevNote.start + newDur, curSpb, curBeatsPerStep);
          newDur = Math.max(MIN_DUR, snappedEnd - drag.prevNote.start);
        }
        setLiveNotes(curNotes.map((n) => n.id === drag.id ? { ...n, duration: newDur } : n));
      }

      if (drag.type === "create") {
        const dx = e.clientX - drag.startX;
        let newDur = Math.max(MIN_DUR, drag.defaultDur + dx / curPps);
        if (curSnap) {
          const snappedEnd = snapRound(drag.startTime + newDur, curSpb, curBeatsPerStep);
          newDur = Math.max(MIN_DUR, snappedEnd - drag.startTime);
        }
        setLiveNotes(curNotes.map((n) => n.id === drag.id ? { ...n, duration: newDur } : n));
      }
    }

    function onUp() {
      const drag = dragRef.current;
      if (drag.type === "idle") return;

      const live = liveNotesRef.current; // always current via ref

      if (drag.type === "move" && live) {
        const moved = live.filter((n) => drag.ids.includes(n.id));
        if (moved.length) {
          useHistoryStore.getState().execute(
            new UpdateMidiNotesCommand(clip.id, drag.prevNotes, moved, "Move MIDI Notes")
          );
        }
      }

      if (drag.type === "resize" && live) {
        const resized = live.find((n) => n.id === drag.id);
        if (resized && Math.abs(resized.duration - drag.prevNote.duration) > 0.001) {
          useHistoryStore.getState().execute(
            new UpdateMidiNotesCommand(clip.id, [drag.prevNote], [resized], "Resize MIDI Note")
          );
        }
      }

      if (drag.type === "create" && live) {
        const created = live.find((n) => n.id === drag.id);
        // Only patch store if user dragged to a different duration
        if (created && Math.abs(created.duration - drag.defaultDur) > 0.001) {
          useProjectStore.getState().updateMidiNotes(clip.id, [created]);
        }
      }

      dragRef.current = { type: "idle" };
      setLiveNotes(null);
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup",   onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup",   onUp);
    };
  }, [clip.id]); // intentionally only clip.id

  // ── Keyboard shortcuts ────────────────────────────────────────────────────
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      const ctrl         = e.ctrlKey || e.metaKey;
      const curNotes     = notesRef.current;
      const curSel       = selIdsRef.current;
      const curSpb       = spbRef.current;
      const curBPS       = GRID_FRACS[gridResRef.current];

      if ((e.key === "Delete" || e.key === "Backspace") && curSel.size > 0) {
        e.preventDefault();
        const toDelete = curNotes.filter((n) => curSel.has(n.id));
        useHistoryStore.getState().execute(new RemoveMidiNotesCommand(clip.id, toDelete));
        setSelIds(new Set());
        return;
      }
      if (ctrl && e.key === "a") {
        e.preventDefault();
        setSelIds(new Set(curNotes.map((n) => n.id)));
        return;
      }
      if (ctrl && e.key === "d" && curSel.size > 0) {
        e.preventDefault();
        const toDup  = curNotes.filter((n) => curSel.has(n.id));
        const maxEnd = Math.max(...toDup.map((n) => n.start + n.duration));
        const span   = maxEnd - Math.min(...toDup.map((n) => n.start));
        const duped: MidiNote[] = toDup.map((n) => ({ ...n, id: crypto.randomUUID(), start: n.start + span }));
        useHistoryStore.getState().execute(new AddMidiNotesCommand(clip.id, duped));
        setSelIds(new Set(duped.map((n) => n.id)));
        return;
      }
      if ((e.key === "ArrowUp" || e.key === "ArrowDown") && curSel.size > 0 && !ctrl) {
        e.preventDefault();
        const dp   = e.key === "ArrowUp" ? 1 : -1;
        const prev = curNotes.filter((n) => curSel.has(n.id)).map((n) => ({ ...n }));
        const next = prev.map((n) => ({ ...n, pitch: Math.max(0, Math.min(127, n.pitch + dp)) }));
        useHistoryStore.getState().execute(new UpdateMidiNotesCommand(clip.id, prev, next, "Transpose"));
        return;
      }
      if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && curSel.size > 0 && !ctrl) {
        e.preventDefault();
        const ds   = (e.key === "ArrowRight" ? 1 : -1) * curSpb * curBPS;
        const prev = curNotes.filter((n) => curSel.has(n.id)).map((n) => ({ ...n }));
        const next = prev.map((n) => ({ ...n, start: Math.max(0, n.start + ds) }));
        useHistoryStore.getState().execute(new UpdateMidiNotesCommand(clip.id, prev, next, "Nudge Notes"));
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [clip.id]);

  // ── Ctrl+wheel zoom ───────────────────────────────────────────────────────
  useEffect(() => {
    const el = mainRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
      setPpb((prev) => Math.max(20, Math.min(400, prev * factor)));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  // ── Quantize ──────────────────────────────────────────────────────────────
  function quantize() {
    const ids  = selIds.size > 0 ? [...selIds] : notes.map((n) => n.id);
    const prev = notes.filter((n) => ids.includes(n.id)).map((n) => ({ ...n }));
    const next = prev.map((n) => ({ ...n, start: snapRound(n.start, spb, GRID_FRACS[gridRes]) }));
    useHistoryStore.getState().execute(new UpdateMidiNotesCommand(clip.id, prev, next, "Quantize"));
  }

  // ── Velocity drag ─────────────────────────────────────────────────────────
  function handleVelMouseDown(e: React.MouseEvent, note: MidiNote) {
    e.stopPropagation();
    const startY   = e.clientY;
    const origVel  = note.velocity;
    const prevNote = { ...note };

    function onMove(ev: MouseEvent) {
      const newVel = Math.max(1, Math.min(127, origVel + Math.round(startY - ev.clientY)));
      useProjectStore.getState().updateMidiNotes(clip.id, [{ id: note.id, velocity: newVel }]);
    }
    function onUp() {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup",   onUp);
      const finalNote = useProjectStore.getState().project.tracks
        .flatMap((t) => t.clips).find((c) => c.id === clip.id)
        ?.notes?.find((n) => n.id === note.id);
      if (finalNote && finalNote.velocity !== prevNote.velocity) {
        useHistoryStore.getState().push(
          new UpdateMidiNotesCommand(clip.id, [prevNote], [finalNote], "Set Velocity")
        );
      }
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup",   onUp);
  }

  // ── Render ────────────────────────────────────────────────────────────────
  return (
    <div className="flex h-full flex-col overflow-hidden" style={{ background: "#0c0f12" }}>

      {/* ── Toolbar ── */}
      <div className="flex h-9 shrink-0 items-center gap-1.5 border-b px-2"
           style={{ borderColor: "rgba(255,255,255,0.07)", background: "#111418" }}>

        <div className="flex items-center gap-0.5 rounded-md border p-0.5"
             style={{ borderColor: "rgba(255,255,255,0.08)", background: "rgba(0,0,0,0.2)" }}>
          <ToolBtn active={tool === "draw"}   onClick={() => setTool("draw")}   title="Draw"><Pencil size={11} /></ToolBtn>
          <ToolBtn active={tool === "select"} onClick={() => setTool("select")} title="Select"><MousePointer2 size={11} /></ToolBtn>
        </div>

        <Divider />

        <ToolBtn active={snapOn} onClick={() => setSnapOn((v) => !v)} title="Snap to grid">
          <Magnet size={11} />
        </ToolBtn>

        {/* Grid resolution dropdown */}
        <div className="relative">
          <button
            onClick={() => setGridOpen((v) => !v)}
            className="flex h-6 items-center gap-1 rounded border px-2 text-[10px] tabular-nums text-daw-dim transition-colors hover:border-white/20 hover:text-daw-text"
            style={{ borderColor: "rgba(255,255,255,0.1)", background: "rgba(255,255,255,0.04)" }}
          >
            {gridRes}<ChevronDown size={9} />
          </button>
          {gridOpen && (
            <div className="absolute left-0 top-7 z-50 rounded border py-0.5 shadow-xl"
                 style={{ background: "#1a1e24", borderColor: "rgba(255,255,255,0.1)", minWidth: 64 }}>
              {GRID_OPTIONS.map((g) => (
                <button key={g} onClick={() => { setGridRes(g); setGridOpen(false); }}
                  className="flex w-full items-center px-3 py-1 text-[10px] tabular-nums text-daw-dim transition-colors hover:bg-white/[0.06] hover:text-daw-text"
                  style={{ background: g === gridRes ? "rgba(255,255,255,0.06)" : undefined }}>
                  {g}
                </button>
              ))}
            </div>
          )}
        </div>

        <Divider />

        <ToolBtn onClick={quantize} title="Quantize" active={false}>
          <span className="text-[9px] font-bold">Q</span>
        </ToolBtn>
        <ToolBtn active={false} title="Duplicate selected (Ctrl+D)"
          onClick={() => {
            const toDup = notes.filter((n) => selIds.has(n.id));
            if (!toDup.length) return;
            const maxEnd = Math.max(...toDup.map((n) => n.start + n.duration));
            const span   = maxEnd - Math.min(...toDup.map((n) => n.start));
            const duped: MidiNote[] = toDup.map((n) => ({ ...n, id: crypto.randomUUID(), start: n.start + span }));
            useHistoryStore.getState().execute(new AddMidiNotesCommand(clip.id, duped));
            setSelIds(new Set(duped.map((n) => n.id)));
          }}>
          <Copy size={11} />
        </ToolBtn>
        <ToolBtn active={false} title="Delete selected (Del)"
          onClick={() => {
            const toDelete = notes.filter((n) => selIds.has(n.id));
            if (!toDelete.length) return;
            useHistoryStore.getState().execute(new RemoveMidiNotesCommand(clip.id, toDelete));
            setSelIds(new Set());
          }}>
          <Trash2 size={11} className="text-red-400/60" />
        </ToolBtn>

        <div className="flex-1" />

        <div className="flex items-center gap-1.5 text-[9px] text-daw-faint">
          <div className="h-2 w-2 rounded-full" style={{ background: trackColor }} />
          <span className="max-w-[14ch] truncate">{clip.name}</span>
          <span className="opacity-50">·</span>
          <span>{notes.length} notes</span>
        </div>

        <div className="flex items-center gap-0.5 rounded border pl-1 pr-0.5 text-[9px] text-daw-faint"
             style={{ borderColor: "rgba(255,255,255,0.07)", background: "rgba(0,0,0,0.15)" }}>
          <span className="tabular-nums">{Math.round(ppb)}px/bt</span>
          <button onClick={() => setPpb((p) => Math.max(20, p / 1.4))} className="flex h-5 w-5 items-center justify-center rounded hover:bg-white/[0.07]">−</button>
          <button onClick={() => setPpb((p) => Math.min(400, p * 1.4))} className="flex h-5 w-5 items-center justify-center rounded hover:bg-white/[0.07]">+</button>
        </div>
      </div>

      {/* ── Body ── */}
      <div className="flex min-h-0 flex-1">

        {/* Left column: piano keys + vel label */}
        <div className="flex w-[60px] shrink-0 flex-col border-r"
             style={{ borderColor: "rgba(255,255,255,0.07)" }}>

          {/* Piano key lane — scroll-synced with main grid (Y only) */}
          <div ref={pianoRef} className="flex-1 overflow-hidden">
            <div style={{ height: TOTAL_H, position: "relative" }}>
              {Array.from({ length: PITCH_CNT }, (_, i) => {
                const pitch = PITCH_CNT - 1 - i; // i=0 → pitch 127 at top
                const black = isBlack(pitch);
                const isC   = pitch % 12 === 0;
                return (
                  <div key={pitch} style={{
                    position: "absolute",
                    top: i * ROW_H, left: 0, width: "100%", height: ROW_H,
                    background: black ? "#0a0c10" : "#1a1e24",
                    borderBottom: "1px solid rgba(255,255,255,0.05)",
                    display: "flex", alignItems: "center", justifyContent: "flex-end", paddingRight: 5,
                  }}>
                    {!black && (
                      <span style={{ fontSize: 8,
                        color: isC ? "rgba(255,255,255,0.70)" : "rgba(255,255,255,0.25)",
                        fontWeight: isC ? 600 : 400 }}>
                        {isC ? noteName(pitch) : NOTE_NAMES[pitch % 12]}
                      </span>
                    )}
                    {black && (
                      <div style={{
                        position: "absolute", right: 0, top: 1, bottom: 1, width: 32,
                        background: "rgba(0,0,0,0.6)", borderLeft: "1px solid rgba(255,255,255,0.06)",
                      }} />
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          {/* Velocity lane label */}
          <div className="flex shrink-0 items-center justify-center border-t"
               style={{ height: VEL_H, borderColor: "rgba(255,255,255,0.07)", background: "#0e1115" }}>
            <span className="text-[8px] font-semibold uppercase tracking-widest text-daw-faint opacity-50"
                  style={{ writingMode: "vertical-rl", transform: "rotate(180deg)" }}>VEL</span>
          </div>
        </div>

        {/* Right column: piano roll grid + velocity lane */}
        <div className="flex min-w-0 flex-1 flex-col">

          {/* Main scroll area — this is the reference element for clientToGrid */}
          <div ref={mainRef} className="flex-1 overflow-auto" onScroll={syncScroll}
               onClick={() => setGridOpen(false)}>
            {/* Inner content: fixed logical size, contains canvas + notes */}
            <div style={{ width: gridW, height: TOTAL_H, position: "relative", userSelect: "none" }}
                 onMouseDown={handleGridMouseDown}>

              {/* Canvas: grid lines + row backgrounds (DPR-scaled internally) */}
              <canvas ref={canvasRef} style={{ position: "absolute", inset: 0, display: "block" }} />

              {/* MIDI notes — positioned in CSS pixels via timeToX / pitchToY */}
              {displayNotes.map((note) => {
                const x   = timeToX(note.start);
                const y   = pitchToY(note.pitch);
                const w   = Math.max(4, durationToWidth(note.duration));
                const sel = selIds.has(note.id);
                return (
                  <div key={note.id} onMouseDown={(e) => handleNoteMouseDown(e, note)}
                    style={{
                      position: "absolute",
                      left: x, top: y + 1, width: w, height: ROW_H - 2,
                      background: sel ? `${trackColor}ff` : `${trackColor}cc`,
                      border: `1px solid ${sel ? "rgba(255,255,255,0.25)" : trackColor + "88"}`,
                      borderRadius: 2,
                      cursor: tool === "select" ? "grab" : "pointer",
                      zIndex: sel ? 3 : 2,
                      boxShadow: sel ? `0 0 0 1px ${trackColor}` : undefined,
                    }}>
                    {w > 24 && (
                      <span style={{
                        position: "absolute", left: 2, top: 1,
                        fontSize: 7, color: "rgba(0,0,0,0.7)", fontWeight: 700,
                        pointerEvents: "none", whiteSpace: "nowrap", overflow: "hidden",
                      }}>
                        {noteName(note.pitch)}
                      </span>
                    )}
                    {/* Right-edge resize handle */}
                    <div
                      onMouseDown={(e) => { e.stopPropagation(); handleNoteMouseDown(e, note, true); }}
                      style={{
                        position: "absolute", right: 0, top: 0, width: 5, height: "100%",
                        cursor: "ew-resize", background: "rgba(255,255,255,0.25)",
                        borderRadius: "0 2px 2px 0",
                      }}
                    />
                  </div>
                );
              })}
            </div>
          </div>

          {/* Velocity lane — scroll-synced with main grid (X only) */}
          <div ref={velRef} className="shrink-0 overflow-hidden border-t"
               style={{ height: VEL_H, borderColor: "rgba(255,255,255,0.07)", background: "#0e1115" }}>
            <div style={{ width: gridW, height: VEL_H, position: "relative" }}>
              {[0.25, 0.5, 0.75].map((f) => (
                <div key={f} style={{
                  position: "absolute", left: 0, right: 0,
                  bottom: f * (VEL_H - 4), height: 1,
                  background: "rgba(255,255,255,0.05)",
                }} />
              ))}
              {displayNotes.map((note) => {
                const x    = timeToX(note.start);
                const barH = Math.round(((note.velocity - 1) / 126) * (VEL_H - 6));
                const barW = Math.max(3, Math.min(8, durationToWidth(note.duration) - 2));
                const sel  = selIds.has(note.id);
                return (
                  <div key={note.id} onMouseDown={(e) => handleVelMouseDown(e, note)}
                    style={{
                      position: "absolute",
                      left: Math.round(x), bottom: 2,
                      width: barW, height: barH,
                      background: sel ? trackColor : `${trackColor}80`,
                      borderRadius: "2px 2px 0 0",
                      cursor: "ns-resize",
                    }}
                  />
                );
              })}
            </div>
          </div>
        </div>
      </div>

      {/* Grid dropdown backdrop */}
      {gridOpen && <div className="fixed inset-0 z-40" onClick={() => setGridOpen(false)} />}
    </div>
  );
}

// ── Shared sub-components ─────────────────────────────────────────────────────
function ToolBtn({
  children, active, onClick, title,
}: {
  children: React.ReactNode; active: boolean; onClick: () => void; title?: string;
}) {
  return (
    <button type="button" title={title} onClick={onClick}
      className="flex h-6 min-w-[24px] items-center justify-center gap-1 rounded px-1.5 transition-colors"
      style={{
        background:  active ? "rgba(255,255,255,0.12)" : "transparent",
        color:       active ? "#e8eef4" : "rgba(180,192,204,0.55)",
        border:      active ? "1px solid rgba(255,255,255,0.12)" : "1px solid transparent",
      }}>
      {children}
    </button>
  );
}

function Divider() {
  return <div className="mx-0.5 h-5 w-px" style={{ background: "rgba(255,255,255,0.07)" }} />;
}
