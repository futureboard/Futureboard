import * as React from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import "./NumberInput.css";

type Align = "left" | "center" | "right";

export type NumberInputProps = {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
  inputClassName?: string;
  align?: Align;
  disabled?: boolean;
  ariaLabel?: string;
  title?: string;
};

function clamp(value: number, min?: number, max?: number): number {
  let next = value;
  if (min !== undefined) next = Math.max(min, next);
  if (max !== undefined) next = Math.min(max, next);
  return next;
}

function decimalsFor(step: number): number {
  const [, decimals = ""] = String(step).split(".");
  return decimals.length;
}

function formatValue(value: number, step: number): string {
  const decimals = decimalsFor(step);
  return decimals > 0 ? value.toFixed(decimals) : String(value);
}

export function NumberInput({
  value,
  onChange,
  min,
  max,
  step = 1,
  className = "",
  inputClassName = "",
  align = "right",
  disabled = false,
  ariaLabel,
  title,
}: NumberInputProps) {
  const [draft, setDraft] = React.useState(() => formatValue(value, step));

  React.useEffect(() => {
    setDraft(formatValue(value, step));
  }, [step, value]);

  const commit = React.useCallback((raw: string) => {
    const parsed = Number(raw);
    const next = Number.isFinite(parsed) ? clamp(parsed, min, max) : value;
    setDraft(formatValue(next, step));
    if (next !== value) onChange(next);
  }, [max, min, onChange, step, value]);

  const nudge = React.useCallback((direction: 1 | -1) => {
    const next = clamp(value + step * direction, min, max);
    setDraft(formatValue(next, step));
    if (next !== value) onChange(next);
  }, [max, min, onChange, step, value]);

  const canStepUp = !disabled && (max === undefined || value < max);
  const canStepDown = !disabled && (min === undefined || value > min);

  return (
    <div
      className={`daw-number-input ${disabled ? "is-disabled" : ""} ${className}`}
      title={title}
    >
      <input
        type="text"
        inputMode={step % 1 === 0 ? "numeric" : "decimal"}
        value={draft}
        disabled={disabled}
        aria-label={ariaLabel}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => commit(draft)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.currentTarget.blur();
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            nudge(1);
          } else if (e.key === "ArrowDown") {
            e.preventDefault();
            nudge(-1);
          }
        }}
        className={`daw-number-input__field ${align} ${inputClassName}`}
      />
      <div className="daw-number-input__steppers" aria-hidden="true">
        <button
          type="button"
          tabIndex={-1}
          disabled={!canStepUp}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => nudge(1)}
          className="daw-number-input__step"
        >
          <ChevronUp size={9} strokeWidth={2} />
        </button>
        <button
          type="button"
          tabIndex={-1}
          disabled={!canStepDown}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => nudge(-1)}
          className="daw-number-input__step"
        >
          <ChevronDown size={9} strokeWidth={2} />
        </button>
      </div>
    </div>
  );
}
