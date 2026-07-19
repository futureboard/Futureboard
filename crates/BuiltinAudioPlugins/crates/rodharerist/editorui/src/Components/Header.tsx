import logoUrl from "../Assets/logo.svg";

type HeaderProps = {
  presetId: string;
  presetName: string;
  testing: boolean;
  onStepPreset: (dir: number) => void;
  onToggleTest: () => void;
};

export function Header({
  presetId,
  presetName,
  testing,
  onStepPreset,
  onToggleTest,
}: HeaderProps) {
  return (
    <header className="header">
      <div className="brand">
        <img src={logoUrl} alt="Rodhareist" />
        <span className="tag">Native</span>
      </div>

      <div className="preset-nav">
        <button
          className="nav-arrow"
          onClick={() => onStepPreset(-1)}
          aria-label="Previous preset"
          type="button"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
          >
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
        <div className="preset-face">
          <div className="id">{presetId}</div>
          <div className="name">{presetName}</div>
        </div>
        <button
          className="nav-arrow"
          onClick={() => onStepPreset(1)}
          aria-label="Next preset"
          type="button"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </button>
      </div>

      <div className="header-right">
        <div className="di-tester">
          <span className="di-label">Test DI</span>
          <button
            className={`di-btn${testing ? " on" : ""}`}
            onClick={onToggleTest}
            type="button"
          >
            <span className="led" />
            {testing ? "Stop" : "Play"}
          </button>
        </div>
        <span className="bpm">110.0 BPM</span>
      </div>
    </header>
  );
}
