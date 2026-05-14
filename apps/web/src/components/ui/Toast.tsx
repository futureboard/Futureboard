import { useEffect, useState } from "react";

type ToastItem = { id: string; message: string; warn?: boolean };

// Lightweight pub/sub — no Zustand overhead for ephemeral notifications.
const _listeners = new Set<(t: ToastItem) => void>();

export function showToast(message: string, warn = false) {
  const t: ToastItem = { id: crypto.randomUUID(), message, warn };
  _listeners.forEach((fn) => fn(t));
}

export function ToastContainer() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  useEffect(() => {
    const handler = (t: ToastItem) => {
      setToasts((prev) => [...prev.slice(-4), t]); // cap at 5 visible
      setTimeout(
        () => setToasts((prev) => prev.filter((x) => x.id !== t.id)),
        2600,
      );
    };
    _listeners.add(handler);
    return () => { _listeners.delete(handler); };
  }, []);

  if (!toasts.length) return null;

  return (
    <div className="pointer-events-none fixed bottom-10 left-1/2 z-[200] flex -translate-x-1/2 flex-col items-center gap-1.5">
      {toasts.map((t) => (
        <div
          key={t.id}
          className="flex items-center gap-2 rounded-lg border px-3 py-1.5 text-[11px] shadow-2xl"
          style={{
            background: "#1e232c",
            borderColor: t.warn ? "rgba(243,201,105,0.3)" : "rgba(255,255,255,0.1)",
            color: t.warn ? "#f3c969" : "rgba(200,212,224,0.85)",
            boxShadow: "0 6px 28px rgba(0,0,0,0.55)",
          }}
        >
          {t.warn && <span className="text-[10px]">⚠</span>}
          {t.message}
        </div>
      ))}
    </div>
  );
}
