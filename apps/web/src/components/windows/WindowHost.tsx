import { createPortal } from "react-dom";
import { useWindowStore } from "../../store/windowStore";
import { FloatingWindow } from "./FloatingWindow";
import { DialogWindow } from "./DialogWindow";
import { ProjectWizard } from "../project/ProjectWizard";
import { UnsavedChangesDialog } from "./UnsavedChangesDialog";
import { SettingsDialog } from "../settings/SettingsDialog";
import { AudioPluginManager } from "../plugins/AudioPluginManager";
import { AddTrackDialog } from "../AddTrackDialog";

export function WindowHost() {
  const { windows, closeWindow, focusWindow, updateWindowBounds } = useWindowStore();

  const floatingWindows = windows.filter((w) => w.kind === "floating");
  const dialogWindows = windows.filter((w) => w.kind === "dialog");

  return createPortal(
    <>
      {floatingWindows.map((win) => (
        <FloatingWindow
          key={win.id}
          window={win}
          onClose={() => closeWindow(win.id)}
          onFocus={() => focusWindow(win.id)}
          onBoundsChange={(bounds) => updateWindowBounds(win.id, bounds)}
        >
          <WindowContent contentType={win.contentType} id={win.id} payload={win.payload} />
        </FloatingWindow>
      ))}

      {dialogWindows.map((win) => (
        <DialogWindow
          key={win.id}
          title={win.title}
          modal={win.modal ?? true}
          width={win.width}
          height={win.height}
          zIndex={win.zIndex}
          onClose={win.closable !== false ? () => closeWindow(win.id) : undefined}
        >
          <WindowContent contentType={win.contentType} id={win.id} payload={win.payload} />
        </DialogWindow>
      ))}
    </>,
    document.body,
  );
}

type ContentProps = {
  contentType: string;
  id: string;
  payload?: Record<string, unknown>;
};

function WindowContent({ contentType, id, payload }: ContentProps) {
  switch (contentType) {
    case "projectWizard":
      return <ProjectWizard windowId={id} />;
    case "unsavedChanges":
      return <UnsavedChangesDialog windowId={id} />;
    case "preferences":
      return (
        <SettingsDialog
          windowId={id}
          initialTab={(payload?.initialTab as "general" | "audio" | "midi" | "project" | "shortcuts" | "appearance" | "advanced") ?? "general"}
        />
      );
    case "pluginManager":
      return <AudioPluginManager windowId={id} />;
    case "addTrack":
      return <AddTrackDialog onClose={() => useWindowStore.getState().closeWindow(id)} external />;
    default:
      return (
        <div className="flex items-center justify-center h-full text-[11px] text-daw-text-muted">
          {contentType}
        </div>
      );
  }
}
