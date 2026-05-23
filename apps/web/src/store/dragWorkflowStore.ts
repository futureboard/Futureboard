import { create } from "zustand";

export type DragPreviewState = {
  isDraggingFiles: boolean;
  fileCount: number;
  targetTrackIndex: number;
  targetBeat: number;
  snappedBeat: number;
  willCreateTracks: number;
  clientX: number;
  clientY: number;
};

type DragWorkflowStats = {
  dragOverEventsPerSecond: number;
  dragPreviewFramesPerSecond: number;
  dragPreviewUpdateMs: number;
  projectMutationsDuringDrag: number;
  nativeSyncDuringDrag: number;
};

type DragWorkflowStore = {
  preview: DragPreviewState | null;
  stats: DragWorkflowStats;
  beginDrag: () => void;
  updatePreview: (preview: DragPreviewState, updateMs: number) => void;
  endDrag: () => void;
  recordDragOverEvent: () => void;
  markProjectMutationDuringDrag: () => void;
  markNativeSyncDuringDrag: () => void;
};

const initialStats: DragWorkflowStats = {
  dragOverEventsPerSecond: 0,
  dragPreviewFramesPerSecond: 0,
  dragPreviewUpdateMs: 0,
  projectMutationsDuringDrag: 0,
  nativeSyncDuringDrag: 0,
};

let dragEventCount = 0;
let dragFrameCount = 0;
let lastFlush = performance.now();

function nextStats(updateMs?: number): Partial<DragWorkflowStats> {
  const now = performance.now();
  if (now - lastFlush < 500) {
    return updateMs == null ? {} : { dragPreviewUpdateMs: updateMs };
  }
  const elapsed = now - lastFlush;
  const stats = {
    dragOverEventsPerSecond: Math.round((dragEventCount * 1000) / elapsed),
    dragPreviewFramesPerSecond: Math.round((dragFrameCount * 1000) / elapsed),
    ...(updateMs == null ? {} : { dragPreviewUpdateMs: updateMs }),
  };
  dragEventCount = 0;
  dragFrameCount = 0;
  lastFlush = now;
  return stats;
}

export const useDragWorkflowStore = create<DragWorkflowStore>((set, get) => ({
  preview: null,
  stats: initialStats,

  beginDrag: () => {
    dragEventCount = 0;
    dragFrameCount = 0;
    lastFlush = performance.now();
    set({ stats: initialStats });
  },

  updatePreview: (preview, updateMs) => {
    dragFrameCount++;
    set((state) => ({
      preview,
      stats: {
        ...state.stats,
        ...nextStats(updateMs),
      },
    }));
  },

  endDrag: () => set({ preview: null }),

  recordDragOverEvent: () => {
    dragEventCount++;
    const patch = nextStats();
    if (Object.keys(patch).length > 0) {
      set((state) => ({ stats: { ...state.stats, ...patch } }));
    }
  },

  markProjectMutationDuringDrag: () => {
    if (!get().preview?.isDraggingFiles) return;
    set((state) => ({
      stats: {
        ...state.stats,
        projectMutationsDuringDrag: state.stats.projectMutationsDuringDrag + 1,
      },
    }));
  },

  markNativeSyncDuringDrag: () => {
    if (!get().preview?.isDraggingFiles) return;
    set((state) => ({
      stats: {
        ...state.stats,
        nativeSyncDuringDrag: state.stats.nativeSyncDuringDrag + 1,
      },
    }));
  },
}));
