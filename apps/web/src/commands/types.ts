/**
 * Base contract for all undoable DAW commands.
 *
 * - `execute()` — apply the change for the first time
 * - `undo()`    — reverse the change
 * - `redo()`    — re-apply after an undo (defaults to execute if not provided)
 * - `label`     — human-readable string shown in Edit → Undo / Redo menus
 * - `id`        — optional stable identifier for deduplication / merging
 */
export interface DawCommand {
  readonly id?: string;
  readonly label: string;
  execute(): void;
  undo(): void;
  redo?(): void;
}
