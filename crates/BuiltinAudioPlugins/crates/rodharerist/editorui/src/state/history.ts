// Undo/redo history and A/B slots.
//
// Deliberately pure and generic over the snapshot type: no React, no DSP, no
// DOM. The Editor owns the side effects (pushing the restored snapshot to the
// DSP); this module only decides *which* snapshot is current.

/** An immutable undo/redo stack. Every operation returns a new history. */
export type History<T> = {
  /** Snapshots older than the present, oldest first. */
  readonly past: readonly T[];
  readonly present: T;
  /** Snapshots newer than the present, nearest first. */
  readonly future: readonly T[];
};

/** Bound on retained snapshots, so a long session cannot grow without limit. */
export const HISTORY_LIMIT = 64;

export function createHistory<T>(present: T): History<T> {
  return { past: [], present, future: [] };
}

/**
 * Record a new present. Drops the redo branch, as any editing action after an
 * undo starts a new one.
 *
 * `isEqual` lets the caller collapse no-op commits (e.g. re-selecting the model
 * that is already active) so undo does not accumulate steps that change nothing.
 */
export function commit<T>(
  history: History<T>,
  next: T,
  isEqual?: (a: T, b: T) => boolean,
): History<T> {
  if (isEqual?.(history.present, next)) return history;
  const past = [...history.past, history.present];
  return {
    past: past.length > HISTORY_LIMIT ? past.slice(past.length - HISTORY_LIMIT) : past,
    present: next,
    future: [],
  };
}

/**
 * Replace the present without creating an undo step. Used when loading a preset
 * establishes a new baseline rather than performing an edit.
 */
export function reset<T>(_history: History<T>, present: T): History<T> {
  return createHistory(present);
}

export function canUndo<T>(history: History<T>): boolean {
  return history.past.length > 0;
}

export function canRedo<T>(history: History<T>): boolean {
  return history.future.length > 0;
}

export function undo<T>(history: History<T>): History<T> {
  if (history.past.length === 0) return history;
  const present = history.past[history.past.length - 1]!;
  return {
    past: history.past.slice(0, -1),
    present,
    future: [history.present, ...history.future],
  };
}

export function redo<T>(history: History<T>): History<T> {
  if (history.future.length === 0) return history;
  const [present, ...rest] = history.future as [T, ...T[]];
  return {
    past: [...history.past, history.present],
    present,
    future: rest,
  };
}

// ---------------------------------------------------------------------------
// A/B compare
// ---------------------------------------------------------------------------

export type AbSlot = "A" | "B";

/**
 * Two complete plugin states with one active. Switching swaps which slot the
 * editor is showing; the inactive slot holds the state it had when it was last
 * active, so A/B compares the *entire* rig, not just one module.
 */
export type AbState<T> = {
  readonly active: AbSlot;
  readonly A: T;
  readonly B: T;
};

export function createAb<T>(initial: T): AbState<T> {
  return { active: "A", A: initial, B: initial };
}

export function other(slot: AbSlot): AbSlot {
  return slot === "A" ? "B" : "A";
}

/** The snapshot currently shown. */
export function activeSnapshot<T>(ab: AbState<T>): T {
  return ab[ab.active];
}

/** Store `current` into the active slot (call before reading/switching). */
export function syncActive<T>(ab: AbState<T>, current: T): AbState<T> {
  return { ...ab, [ab.active]: current };
}

/**
 * Switch slots. `current` is written into the outgoing slot first, so edits made
 * since the last switch are not lost.
 */
export function switchSlot<T>(ab: AbState<T>, current: T): AbState<T> {
  const synced = syncActive(ab, current);
  return { ...synced, active: other(ab.active) };
}

/** Copy the active slot over the inactive one (A→B or B→A). */
export function copyToOther<T>(ab: AbState<T>, current: T): AbState<T> {
  const synced = syncActive(ab, current);
  return { ...synced, [other(ab.active)]: synced[ab.active] };
}

/** Set which slot is active without touching either stored snapshot. */
export function setActive<T>(ab: AbState<T>, slot: AbSlot, current: T): AbState<T> {
  if (ab.active === slot) return ab;
  return switchSlot(ab, current);
}
