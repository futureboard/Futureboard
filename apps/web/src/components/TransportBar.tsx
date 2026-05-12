import { Circle, FolderOpen, PanelBottom, PanelRight, Pause, Play, Redo2, Save, Share2, SkipBack, Square, Undo2 } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useTransportStore } from "../store/transportStore";
import { useUIStore } from "../store/uiStore";
import { transport } from "../engine/Transport";
import { clipScheduler } from "../engine/ClipScheduler";

function formatTime(t: number): string {
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  const ds = Math.floor((t % 1) * 10);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${ds}`;
}

function Divider() {
  return <div className="mx-0.5 h-6 w-px shrink-0 bg-daw-border" />;
}

function IconBtn({
  icon: Icon, label, onClick, active = false, accent = false, danger = false, disabled = false, size = 14,
}: {
  icon: React.ElementType; label: string; onClick?: () => void;
  active?: boolean; accent?: boolean; danger?: boolean; disabled?: boolean; size?: number;
}) {
  const cls = [
    "flex h-7 w-7 shrink-0 items-center justify-center rounded border transition-colors disabled:opacity-30",
    danger && active   ? "border-daw-red bg-daw-red text-daw-ink hover:bg-daw-red"
    : accent && active ? "border-daw-accent bg-daw-accent text-daw-ink hover:bg-daw-accent-h"
    : active           ? "border-daw-border-light bg-daw-surface-higher text-daw-text hover:bg-daw-border"
    :                    "border-transparent bg-transparent text-daw-dim hover:border-daw-border hover:bg-daw-surface-high hover:text-daw-text",
  ].join(" ");
  return (
    <button onClick={onClick} disabled={disabled} title={label} className={cls}>
      <Icon size={size} />
    </button>
  );
}

export function TransportBar({ onImport, onSave }: { onImport?: () => void; onSave?: () => void }) {
  const { isPlaying, playheadTime, setIsPlaying } = useTransportStore();
  const { project, setBpm } = useProjectStore();
  const { inspectorOpen, toggleInspector, mixerOpen, toggleMixer } = useUIStore();

  const handlePlay = async () => {
    await transport.play(() => { clipScheduler.schedule(project.tracks); setIsPlaying(true); });
  };
  const handlePause = () => { transport.pause(); clipScheduler.cancelAll(); setIsPlaying(false); };
  const handleStop  = () => { transport.stop(() => { clipScheduler.cancelAll(); setIsPlaying(false); }); };

  return (
    <div className="flex h-10 shrink-0 select-none items-center gap-1.5 border-b border-daw-border bg-daw-sunken px-2.5">
      <div className="mr-1 flex min-w-48 shrink-0 items-center gap-2 border-r border-daw-border pr-2.5">
        <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded bg-daw-accent text-daw-ink">
          <span className="text-[13px] font-black leading-none">M</span>
        </div>
        <div className="min-w-0">
          <div className="truncate text-[13px] font-semibold text-daw-text">{project.name}</div>
          <div className="text-[10px] text-daw-faint">Saved locally</div>
        </div>
      </div>

      <IconBtn icon={Undo2} label="Undo" disabled />
      <IconBtn icon={Redo2} label="Redo" disabled />

      <Divider />

      <IconBtn icon={PanelBottom} label="Toggle Mixer" active={mixerOpen} onClick={toggleMixer} />
      <IconBtn icon={PanelRight} label="Toggle Inspector" active={inspectorOpen} onClick={toggleInspector} />

      <Divider />

      <IconBtn icon={SkipBack} label="Return to start" onClick={() => { transport.seek(0); clipScheduler.cancelAll(); }} />
      {isPlaying
        ? <IconBtn icon={Pause}  label="Pause" active onClick={handlePause} />
        : <IconBtn icon={Play}   label="Play"         onClick={handlePlay}  size={15} />
      }
      <IconBtn icon={Square} label="Stop"   onClick={handleStop} disabled={!isPlaying && playheadTime === 0} />
      <IconBtn icon={Circle} label="Record" accent danger size={12} />

      <Divider />

      <div className="flex h-7 min-w-[6.75rem] items-center justify-center rounded border border-daw-border bg-daw-bg px-2.5 text-[15px] font-semibold tabular-nums text-daw-green">
        {formatTime(playheadTime)}
      </div>

      <Divider />

      <div className="flex h-7 items-center gap-2 rounded border border-daw-border bg-daw-bg px-2">
        <span className="text-[10px] font-medium text-daw-faint">BPM</span>
        <input
          type="number" min={20} max={300} value={project.bpm}
          onChange={(e) => setBpm(parseInt(e.target.value) || 120)}
          className="w-10 border-none bg-transparent text-center text-[13px] font-semibold text-daw-text outline-none"
        />
      </div>

      <div className="flex h-7 items-center gap-1 rounded border border-daw-border bg-daw-bg px-2">
        <span className="text-[13px] font-semibold text-daw-dim">4</span>
        <div className="mx-0.5 h-3 w-px bg-daw-border" />
        <span className="text-[13px] font-semibold text-daw-dim">4</span>
      </div>

      <div className="flex-1" />

      <IconBtn icon={FolderOpen} label="Import Audio" onClick={onImport} />
      <IconBtn icon={Save}       label="Save Project" onClick={onSave} />
      <IconBtn icon={Share2} label="Share" disabled />
    </div>
  );
}
