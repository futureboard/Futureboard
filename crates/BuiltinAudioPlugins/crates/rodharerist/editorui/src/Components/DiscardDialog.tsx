type DiscardDialogProps = {
  presetName: string;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
};

export function DiscardDialog({
  presetName,
  onSave,
  onDiscard,
  onCancel,
}: DiscardDialogProps) {
  return (
    <div className="modal-backdrop" role="presentation" onClick={onCancel}>
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="discard-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div id="discard-title" className="modal-title">
          Unsaved changes
        </div>
        <p className="modal-body">
          <strong>{presetName}</strong> has edits that are not saved. Save
          before switching, discard them, or cancel.
        </p>
        <div className="modal-actions">
          <button type="button" className="rig-btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="rig-btn" onClick={onDiscard}>
            Discard
          </button>
          <button
            type="button"
            className="rig-btn primary ready"
            onClick={onSave}
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
