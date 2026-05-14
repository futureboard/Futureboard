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
  modal?: boolean;
  width?: number;
  height?: number;
  onClose?: () => void;
  zIndex?: number;
};

export function DialogWindow({ title, children, actions, modal = true, width = 480, height, onClose, zIndex = 2000 }: Props) {
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = dialogRef.current?.querySelector<HTMLElement>("[autofocus], button[data-autofocus]");
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
    if (v === "primary") return "bg-blue-600 hover:bg-blue-500 text-white";
    if (v === "danger") return "bg-red-700 hover:bg-red-600 text-white";
    return "bg-daw-surface hover:bg-white/10 text-daw-text border border-daw-border";
  };

  return (
    <>
      {/* Backdrop */}
      {modal && (
        <div
          className="fixed inset-0 bg-black/50"
          style={{ zIndex: zIndex - 1 }}
          onClick={onClose}
        />
      )}

      {/* Dialog */}
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal={modal}
        aria-label={title}
        className="fixed flex flex-col bg-daw-panel border border-daw-border shadow-2xl"
        style={{
          zIndex,
          width,
          ...(height ? { height } : {}),
          left: "50%",
          top: "50%",
          transform: "translate(-50%, -50%)",
        }}
      >
        {/* Titlebar */}
        {title && (
          <div className="flex items-center h-8 px-3 bg-daw-surface border-b border-daw-border flex-shrink-0">
            <span className="flex-1 text-[11px] font-semibold text-daw-text uppercase tracking-wide">{title}</span>
            {onClose && (
              <button
                className="w-5 h-5 flex items-center justify-center rounded text-daw-text-muted hover:text-daw-text hover:bg-white/10 text-xs"
                onClick={onClose}
                title="Close"
              >
                ✕
              </button>
            )}
          </div>
        )}

        {/* Content */}
        <div className="flex-1 overflow-auto p-4 min-h-0">
          {children}
        </div>

        {/* Footer actions */}
        {actions && actions.length > 0 && (
          <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-daw-border bg-daw-surface flex-shrink-0">
            {actions.map((a) => (
              <button
                key={a.label}
                className={`px-3 py-1.5 text-[11px] font-medium rounded ${variantClass(a.variant)}`}
                onClick={a.onClick}
                data-autofocus={a.autoFocus ? "" : undefined}
                // eslint-disable-next-line jsx-a11y/no-autofocus
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
