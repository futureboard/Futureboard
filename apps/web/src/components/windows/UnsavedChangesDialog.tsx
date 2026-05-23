import { useWindowStore } from "../../store/windowStore";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { platform } from "../../platform";

type Props = { windowId: string };

export function UnsavedChangesDialog({ windowId: _ }: Props) {
  const ws = useWindowStore.getState;

  const handleSave = async () => {
    const store = useWindowStore.getState();
    const continuation = store.consumePendingAction();
    store.closeAllDialogs();

    try {
      await platform.projectStorage.saveProject(useProjectStore.getState().project);
      useUIStore.getState().setSaveStatus("saved");
    } catch (e) {
      console.warn("[UnsavedChangesDialog] save failed:", e);
    }

    continuation?.();
  };

  const handleDontSave = () => {
    const store = useWindowStore.getState();
    const continuation = store.consumePendingAction();
    store.closeAllDialogs();
    continuation?.();
  };

  const handleCancel = () => {
    const store = useWindowStore.getState();
    store.setPendingAction(null);
    store.closeAllDialogs();
  };

  // suppress the unused variable warning from the _ rename
  void ws;

  return (
    <div className="flex flex-col gap-4 text-[12px] p-4">
      <p className="text-daw-text">
        You have unsaved changes in this project. Do you want to save before continuing?
      </p>
      <p className="text-[11px] text-daw-text-muted">
        If you don't save, your changes will be lost.
      </p>

      <div className="flex gap-2 justify-end border-t border-daw-border pt-3">
        <button
          className="px-3 py-1.5 text-[11px] bg-red-900/60 hover:bg-red-700/80 text-red-200 border border-red-800/50 rounded"
          onClick={handleDontSave}
        >
          Don't Save
        </button>
        <button
          className="px-3 py-1.5 text-[11px] bg-daw-surface hover:bg-white/10 text-daw-text border border-daw-border rounded"
          onClick={handleCancel}
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
        >
          Cancel
        </button>
        <button
          className="px-3 py-1.5 text-[11px] bg-blue-600 hover:bg-blue-500 text-white rounded font-medium"
          onClick={() => void handleSave()}
        >
          Save
        </button>
      </div>
    </div>
  );
}
