import logoUrl from "../Assets/logo.svg";

type HeaderProps = {
  presetId: string;
  presetName: string;
  modified: boolean;
  testing: boolean;
  showTestDi: boolean;
  onStepPreset: (dir: number) => void;
  onToggleTest: () => void;
  onSave: () => void;
  onRevert: () => void;
};

export function Header({
  presetId,
  presetName,
  modified,
  testing,
  showTestDi,
  onStepPreset,
  onToggleTest,
  onSave,
  onRevert,
}: HeaderProps) {
  return (
    <header className="header">
      <div className="brand">
        <img src={logoUrl} alt="Rodhareist" />
        <span className="tag">Native</span>
      </div>

      <div className="preset-nav" aria-label="Rig selector">
        <button
          className="nav-arrow"
          onClick={() => onStepPreset(-1)}
          aria-label="Previous preset"
          type="button"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.4"
          >
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>

        <div className="rig-lcd">
          <div className="rig-lcd-top">
            <span className="rig-badge">Rig</span>
            <span className="rig-slot">{presetId}</span>
            {modified && <span className="mod-dot" title="Modified" />}
          </div>
          <div className="rig-name">
            {presetName}
            {modified ? " *" : ""}
          </div>
        </div>

        <button
          className="nav-arrow"
          onClick={() => onStepPreset(1)}
          aria-label="Next preset"
          type="button"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.4"
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </button>
      </div>

      <div className="header-right">
        <div className="rig-actions">
          <button
            type="button"
            className="rig-btn"
            disabled={!modified}
            onClick={onRevert}
            title="Revert to last saved rig"
          >
            Revert
          </button>
          <button
            type="button"
            className={`rig-btn primary${modified ? " ready" : ""}`}
            disabled={!modified}
            onClick={onSave}
            title="Save current rig edits"
          >
            Save
          </button>
        </div>
        {showTestDi && (
          <div className="di-tester">
            <span className="di-label">Test DI</span>
            <button
              className={`di-btn${testing ? " on" : ""}`}
              onClick={onToggleTest}
              type="button"
              title="Preview tone + meters (browser only)"
            >
              <span className="led" />
              {testing ? "Stop" : "Play"}
            </button>
          </div>
        )}
      </div>
    </header>
  );
}
