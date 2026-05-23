/**
 * Arrangement timeline z-index contract.
 *
 * One source of truth for every layer inside the outer <Timeline> container.
 * Every overlay, lane, clip, header, and the playhead is sourced from this
 * map so layers can never silently shadow each other when new UI is added.
 *
 * Stacking-context cheat sheet (read before changing a value):
 *
 *   outer <Timeline>             — position: relative, no z-index, no SC*
 *     └─ TimelineRuler           — no SC at the root; sticky lane creates SC
 *     └─ body wrap               — position: relative, no z-index, no SC
 *          ├─ TimelineGrid       — absolute + z-index → SC (grid)
 *          ├─ FloatingToolsBar   — absolute + z-index → SC (floatingTools)
 *          └─ scrollRef          — absolute + z-index → SC (scrollArea)
 *                ├─ TrackList    (children's z-index is bounded by scrollArea)
 *                ├─ TrackHeader  (sticky + z-index → bounded SC)
 *                └─ Clips        (bounded by scrollArea SC)
 *     └─ Playhead                — absolute + z-index → SC (playhead)
 *     └─ Zoom controls           — absolute + z-index → SC
 *     └─ Drop-highlight / Modal  — absolute + z-index → SC
 *
 *   *SC = stacking context.  All children inside scrollRef (z=scrollArea)
 *   are clamped under any sibling whose z-index is greater than `scrollArea`.
 *
 * Layering rules
 *  • Playhead line and marker MUST share the same z-index.
 *  • Playhead z-index MUST be strictly greater than `rulerHeaderLane` and
 *    `scrollArea` (so it paints over track headers and clips).
 *  • Floating tools, popovers, dialogs, drop highlight MUST be strictly
 *    greater than `playhead`.
 *  • Values inside `scrollArea` (clips, lanes, etc.) are relative to that
 *    nested stacking context — they do not compete with outer values.
 */
export const TIMELINE_Z = {
  // ── inside scrollRef's nested stacking context ──────────────────────────
  /** Track-lane background row. */
  lane:             0,
  /** Audio/MIDI clip body. */
  clip:             5,
  /** Clip resize/fade handles. */
  clipHandle:      10,
  /** Multi-selection / marquee overlay drawn over clips. */
  selectionOverlay: 15,
  /** Sticky-left track header column. */
  trackHeader:     50,

  // ── outer <Timeline> stacking context ───────────────────────────────────
  /** Canvas grid below everything. */
  grid:             0,
  /** Whole scroll area (clamps every child above). */
  scrollArea:      10,
  /** Loop region body in the ruler (escapes wrapRef into outer SC). */
  loopRegion:      15,
  /** Loop handles in the ruler. */
  loopHandle:      18,
  /** Sticky HEADER_WIDTH lane inside the ruler. */
  rulerHeaderLane: 25,
  /** Playhead line + marker — must share the same value. */
  playhead:        40,
  /** Floating arrangement tools (above playhead, below modals). */
  floatingTools:   50,
  /** Zoom controls. */
  zoomControls:    50,
  /** Drop highlight / modals. */
  modal:           60,
} as const;
