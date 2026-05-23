import { useEffect, useRef } from "react";

type DialogAction = {
  label: string;
  onClick: () => void;
  variant?: "primary" | "danger" | "secondary";
  autoFocus?: boolean;
};

type Props = {
  title?: string;
  children: React.ReactNode;
  actions?: DialogAction[];
  /**
   * modal=true = block interaction with app using invisible/non-dark overlay if needed.
   * Default false because Futureboard dialogs should feel like floating native windows.
   */
  modal?: boolean;
  width?: number;
  height?: number;
  onClose?: () => void;
  zIndex?: number;
};

export function DialogWindow({
  title,
  children,
  actions,
  modal = false,
  width = 480,
  height,
  onClose,
  zIndex = 2000,
}: Props) {
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = dialogRef.current?.querySelector<HTMLElement>(
      "[autofocus], button[data-autofocus]"
    );
    el?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };

    document.addEventListener("keydown", onKey, { capture: true });
    return () => document.removeEventListener("keydown", onKey, { capture: true });
  }, [onClose]);

  const variantClass = (v?: string) => {
    if (v === "primary") {
      return "bg-blue-600/90 hover:bg-blue-500 text-white border border-blue-400/20";
    }

    if (v === "danger") {
      return "bg-red-700/90 hover:bg-red-600 text-white border border-red-400/20";
    }

    return "bg-white/[0.045] hover:bg-white/[0.075] text-daw-text border border-white/10";
  };

  return (
    <>
      {/* Invisible interaction blocker only when modal is explicitly needed.
          No dark/tinted backdrop. */}
      {modal && (
        <div
          className="fixed inset-0 bg-transparent"
          style={{ zIndex: zIndex - 1 }}
        />
      )}

      <div
        ref={dialogRef}
        role="dialog"
        aria-modal={modal}
        aria-label={title}
        className="
          fixed flex flex-col overflow-hidden
          rounded-xl
          border border-white/10
          bg-[#11161d]/95
          text-daw-text
          shadow-[0_24px_80px_rgba(0,0,0,0.58),0_0_0_1px_rgba(0,0,0,0.35),0_0_42px_rgba(114,215,215,0.045)]
          backdrop-blur-xl
        "
        style={{
          zIndex,
          width,
          ...(height ? { height } : {}),
          left: "50%",
          top: "50%",
          transform: "translate(-50%, -50%)",
        }}
      >
        {title && (
          <div
            className="
              flex h-8 shrink-0 items-center
              border-b border-white/[0.075]
              bg-[#171c24]/95
              px-3
            "
          >
            <span
              className="
                flex-1 text-[11px] font-semibold uppercase tracking-wide
                text-daw-text
              "
            >
              {title}
            </span>

            {onClose && (
              <button
                className="
                  flex h-5 w-5 items-center justify-center
                  rounded-md
                  text-xs text-daw-text-muted
                  hover:bg-white/10 hover:text-daw-text
                "
                onClick={onClose}
                title="Close"
              >
                ✕
              </button>
            )}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-auto">
          {children}
        </div>

        {actions && actions.length > 0 && (
          <div
            className="
              flex shrink-0 items-center justify-end gap-2
              border-t border-white/[0.075]
              bg-[#171c24]/95
              px-4 py-3
            "
          >
            {actions.map((a) => (
              <button
                key={a.label}
                className={`
                  rounded-md px-3 py-1.5
                  text-[11px] font-medium
                  transition-colors
                  ${variantClass(a.variant)}
                `}
                onClick={a.onClick}
                data-autofocus={a.autoFocus ? "" : undefined}
                autoFocus={a.autoFocus}
              >
                {a.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </>
  );
}
