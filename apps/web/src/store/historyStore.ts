/**
 * historyStore — Zustand store for the undo/redo command stack.
 *
 * Usage:
 *   import { useHistoryStore } from "../store/historyStore";
 *
 *   // Execute a command and push it onto the stack:
 *   useHistoryStore.getState().execute(new AddTrackCommand(track));
 *
 *   // Undo / redo:
 *   useHistoryStore.getState().undo();
 *   useHistoryStore.getState().redo();
 */

import { create } from "zustand";
import type { DawCommand } from "../commands/types";

const MAX_HISTORY = 100;

type HistoryStore = {
  /** Commands that have been executed and can be undone. */
  undoStack: DawCommand[];
  /** Commands that have been undone and can be redone. */
  redoStack: DawCommand[];

  /** Execute a command, push it onto the undo stack, and clear the redo stack. */
  execute: (cmd: DawCommand) => void;

  /**
   * Push a command whose action has ALREADY been applied (e.g. live drag).
   * Does NOT call execute() — just registers the command for undo/redo.
   */
  push: (cmd: DawCommand) => void;

  /** Undo the most recent command. */
  undo: () => void;

  /** Redo the most recently undone command. */
  redo: () => void;

  /** Label of the next command to undo (undefined if stack is empty). */
  undoLabel: () => string | undefined;

  /** Label of the next command to redo (undefined if stack is empty). */
  redoLabel: () => string | undefined;

  /** Clear both stacks (e.g. when loading a new project). */
  clear: () => void;
};

export const useHistoryStore = create<HistoryStore>((set, get) => ({
  undoStack: [],
  redoStack: [],

  execute: (cmd) => {
    cmd.execute();
    set((s) => {
      const next = [...s.undoStack, cmd];
      if (next.length > MAX_HISTORY) next.shift();
      return { undoStack: next, redoStack: [] };
    });
  },

  push: (cmd) => {
    set((s) => {
      const next = [...s.undoStack, cmd];
      if (next.length > MAX_HISTORY) next.shift();
      return { undoStack: next, redoStack: [] };
    });
  },

  undo: () => {
    const { undoStack } = get();
    if (undoStack.length === 0) return;
    const cmd = undoStack[undoStack.length - 1];
    cmd.undo();
    set((s) => ({
      undoStack: s.undoStack.slice(0, -1),
      redoStack: [...s.redoStack, cmd],
    }));
  },

  redo: () => {
    const { redoStack } = get();
    if (redoStack.length === 0) return;
    const cmd = redoStack[redoStack.length - 1];
    (cmd.redo ?? cmd.execute).call(cmd);
    set((s) => ({
      undoStack: [...s.undoStack, cmd],
      redoStack: s.redoStack.slice(0, -1),
    }));
  },

  undoLabel: () => {
    const { undoStack } = get();
    return undoStack.length > 0 ? undoStack[undoStack.length - 1].label : undefined;
  },

  redoLabel: () => {
    const { redoStack } = get();
    return redoStack.length > 0 ? redoStack[redoStack.length - 1].label : undefined;
  },

  clear: () => set({ undoStack: [], redoStack: [] }),
}));
