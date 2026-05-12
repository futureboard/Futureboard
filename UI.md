# ui.md

## UI Direction

This project is a modern browser-based DAW with a clean, focused, professional interface.

The UI may take inspiration from the feeling of Zed Editor: fast, minimal, calm, dense-but-readable, panel-based, keyboard-friendly, and polished.

However, the UI must be unique.

Do not copy Zed Editor directly.
Do not clone its layout, colors, icons, tabs, or exact interaction patterns.
Use it only as a reference for quality, restraint, spacing, clarity, and visual discipline.

The final product should feel like a modern creative tool for audio production, not a generic admin dashboard.

---

## Core UI Goals

The UI must feel:

- Fast
- Sharp
- Minimal
- Premium
- Calm
- Focused
- Spacious
- Professional
- Keyboard-friendly
- Suitable for long creative sessions

The UI must not feel:

- Cheap
- Random
- Crowded
- Bootstrap-like
- Admin-dashboard-like
- Toy-like
- Generic SaaS-like
- Overly colorful
- Visually noisy
- Inconsistent

This is a DAW, not a CRUD panel.

---

## Product Feel

The interface should feel like:

```txt
A high-end code editor
+ a professional audio workstation
+ a cloud-native creative tool
````

It should not feel like:

```txt
A music website
A generic file manager
A random React dashboard
A beginner Tailwind template
A mobile-first SaaS admin panel
```

The UI should look intentional at every pixel.

---

## Visual Inspiration

Acceptable inspiration areas:

```txt
Zed Editor
- calm workspace
- excellent spacing discipline
- focused panels
- clean command/control surfaces
- sharp typography
- premium dark interface feel
```

But the DAW must have its own identity:

```txt
Unique DAW arrangement timeline
Unique clip styling
Unique transport controls
Unique mixer design
Unique browser panel
Unique project workspace
Unique color accents
```

Do not replicate Zed’s exact visual language.

---

## Layout Philosophy

The app should be panel-based.

Recommended main layout:

```txt
┌──────────────────────────────────────────────────────────────┐
│ Top Command Bar / Transport / Project Status                 │
├───────────────┬──────────────────────────────────────────────┤
│ Browser Panel │ Arrangement Timeline                         │
│               │ Tracks / Clips / Waveforms                   │
├───────────────┴──────────────────────────────────────────────┤
│ Mixer / Inspector / Device Panel                             │
└──────────────────────────────────────────────────────────────┘
```

The layout must feel stable.

Panels should not jump, resize randomly, or collapse awkwardly.

Use clear hierarchy:

```txt
Primary focus:
- Timeline
- Tracks
- Clips
- Playhead

Secondary:
- Browser
- Inspector
- Mixer

Tertiary:
- Status text
- Metadata
- Small controls
```

---

## Must-Have UI Regions

### 1. Top Command Bar

Purpose:

* Project name
* Save status
* Undo / redo
* Transport controls
* BPM
* Time display
* Global actions

Should feel compact but not cramped.

Bad:

```txt
Tiny buttons smashed together with no rhythm.
```

Good:

```txt
Grouped controls with clear spacing and alignment.
```

Suggested groups:

```txt
[Project] [Undo Redo] [Transport] [Time] [BPM] [Export / Share]
```

---

### 2. Browser Panel

Purpose:

* Audio files
* Samples
* Project assets
* Search
* Future instruments/effects

The browser should feel like a creative asset panel, not a folder dump.

Important:

* Use clean list rows
* Use good hover states
* Show file duration
* Show type badges subtly
* Search must be visually polished
* Empty state must not look lazy

---

### 3. Arrangement Timeline

This is the main workspace.

It must receive the most visual polish.

Must include:

* Timeline ruler
* Beat/time grid
* Track lanes
* Audio clips
* Waveforms
* Playhead
* Selection states
* Drag states
* Resize handles
* Snap indicators

Timeline must not look like random rectangles inside divs.

Every clip should feel like an audio object.

---

### 4. Track Headers

Track headers should include:

* Track name
* Mute
* Solo
* Arm later
* Volume mini-control
* Pan later
* Level indicator later

Track headers must align perfectly with track lanes.

No vertical drift.
No broken row heights.
No weird inconsistent padding.

---

### 5. Mixer / Inspector Panel

Bottom panel can switch between:

```txt
Mixer
Inspector
Device Chain
Automation
```

For v0.1, Mixer and Inspector are enough.

The mixer must not look like a form.

It should feel like a compact audio tool.

---

## Spacing Rules

Spacing is not optional.

Bad spacing is a bug.

Do not create UI where elements touch edges, float randomly, or feel pasted together.

Use a consistent spacing scale:

```txt
2px  - hairline details only
4px  - tight internal spacing
6px  - compact grouped controls
8px  - standard control spacing
12px - panel padding
16px - major section padding
24px - large breathing room
32px - rare layout separation
```

Recommended default panel padding:

```txt
12px or 16px
```

Recommended compact control gap:

```txt
6px or 8px
```

Recommended panel gap:

```txt
1px border or 8px spacing depending on layout
```

Never use random values like:

```txt
7px
13px
19px
23px
```

Unless there is a specific visual reason.

---

## Padding Requirements

All panels must have padding.

All buttons must have padding.

All inputs must have padding.

All list rows must have padding.

All menus must have padding.

All cards or floating surfaces must have padding.

Do not allow text to touch borders.

Minimum comfortable padding:

```txt
Buttons:        6px 10px
Inputs:         8px 10px
Panel content:  12px
List rows:      8px 10px
Floating menu:  6px
Modal:          20px
```

If a component looks like the text is suffocating, fix padding immediately.

---

## Density

The app should be dense but not cramped.

DAWs need information density, but every element still needs breathing room.

Use compact controls only when:

* Alignment is clean
* Text remains readable
* Hit targets are still usable
* Groups are visually separated

Avoid oversized SaaS spacing.

Avoid microscopic pro-audio chaos.

Aim for:

```txt
Professional compact
not
Crowded hacker prototype
```

---

## Typography

Typography must be clean and consistent.

Recommended font stack:

```css
font-family:
  Inter,
  "SF Pro Display",
  "SF Pro Text",
  system-ui,
  -apple-system,
  BlinkMacSystemFont,
  "Segoe UI",
  sans-serif;
```

For Thai support if needed:

```css
font-family:
  Inter,
  Sarabun,
  "Noto Sans Thai",
  system-ui,
  sans-serif;
```

Use font sizes intentionally:

```txt
11px - tiny metadata only
12px - compact labels
13px - default UI text
14px - readable body/control text
16px - section titles
20px - page/project titles
```

Do not use huge typography unless there is a strong reason.

DAW UI should prioritize workspace, not marketing headings.

---

## Text Rules

Text must be:

* Short
* Clear
* Functional
* Not verbose
* Not decorative

Bad labels:

```txt
Click here to import your beautiful audio file
```

Good labels:

```txt
Import Audio
```

Bad empty state:

```txt
There is nothing here at the moment, please upload something to begin using this feature.
```

Good empty state:

```txt
No audio files yet.
Import a file to start arranging.
```

---

## Color Direction

Use a dark interface first.

The dark theme should feel premium and calm.

Recommended color roles:

```txt
Background base
Panel background
Elevated surface
Border
Muted text
Primary text
Accent
Danger
Warning
Success
Selection
Waveform
Clip body
Playhead
Grid line
```

Avoid using too many saturated colors.

The accent color should be used intentionally for:

* Active state
* Selection
* Playhead
* Primary action
* Focus ring

Do not use accent color everywhere.

---

## Dark Theme Target

Suggested visual mood:

```txt
Deep charcoal
Soft black
Subtle borders
Low-contrast panels
Clear but gentle text
Small accent highlights
Polished hover states
```

Avoid:

```txt
Pure black everywhere
Pure white text everywhere
Neon chaos
Random gradients
Uncontrolled transparency
Washed-out gray soup
```

Good UI needs contrast, not eye violence.

---

## Borders

Use borders carefully.

Borders should separate panels and controls without making the UI look boxed-in.

Recommended:

```txt
1px subtle borders
Low-opacity border color
Slightly brighter border on hover/focus
```

Avoid heavy borders around everything.

Panel separators may use:

```txt
1px solid border
or
subtle shadow
or
slightly different background
```

Do not combine all three everywhere.

---

## Radius

Use moderate border radius.

Recommended:

```txt
Small controls: 6px
Buttons:        8px
Panels:         10px to 14px
Clips:          6px to 10px
Modals:         16px
```

Do not make everything pill-shaped.

Do not make everything square.

The UI should feel modern, not bubbly.

---

## Shadows

Use shadows only for elevated surfaces:

* Menus
* Modals
* Floating panels
* Dragged clips
* Popovers

Do not put heavy shadows on every panel.

Main workspace should feel flat, clean, and precise.

---

## Timeline Visual Rules

The timeline is the soul of the app.

It must look polished.

### Grid

Grid lines should be subtle.

Use stronger lines for major divisions and softer lines for minor divisions.

Do not make the grid dominate the clips.

### Playhead

The playhead must be visually clear.

It should stand above the grid and clips.

Use:

```txt
thin vertical line
small top handle
accent color
high z-index
```

### Clips

Audio clips must include:

* Clip name
* Waveform
* Clear bounds
* Selected state
* Hover state
* Resize handles
* Optional gain indicator later

Clip visual requirements:

```txt
Readable label
Visible waveform
Clear start/end
Subtle background
Good selected state
No ugly default rectangle look
```

Clip selection should feel satisfying and obvious.

---

## Waveform Rules

Waveforms should be drawn with Canvas.

Do not render waveform samples as thousands of DOM elements.

Waveform should:

* Fit inside clip padding
* Respect clip radius
* Have enough contrast
* Not overpower text
* Scale with zoom
* Stay readable at small heights

When zoomed out, simplify the waveform.

When zoomed in, show more detail.

---

## Controls

Buttons should feel tactile and precise.

Button states required:

* Default
* Hover
* Active
* Focus
* Disabled
* Selected / toggled

Do not rely only on color.

Use shape, opacity, border, or icon changes too.

Control height guidance:

```txt
Compact: 28px
Default: 32px
Large:   36px
```

Avoid random button sizes.

---

## Icons

Use icons sparingly.

Icons must be:

* Consistent
* Same stroke width
* Same visual family
* Properly aligned
* Not decorative spam

Recommended icon style:

```txt
Lucide-style outline icons
or
custom minimal DAW icons
```

Do not mix multiple icon packs randomly.

Transport icons must be instantly recognizable:

```txt
Play
Pause
Stop
Record later
Loop
Skip
Metronome later
```

---

## Inputs

Inputs must not look like raw browser defaults.

All inputs require:

* Background
* Border
* Padding
* Radius
* Focus state
* Placeholder style
* Disabled state

Search input in Browser Panel should be polished.

BPM and numeric fields should be compact and aligned.

---

## Menus and Popovers

Menus should be compact but padded.

Menu item height:

```txt
28px to 32px
```

Menu item padding:

```txt
6px 10px
```

Each menu item needs:

* Hover state
* Disabled state
* Optional shortcut label
* Optional icon

Menus must have:

* Border
* Background
* Shadow
* Radius
* Padding

No raw unstyled dropdowns.

---

## Empty States

Empty states must be designed.

Bad:

```txt
Blank panel
```

Good:

```txt
A short message
A small icon or visual marker
One clear action
```

Example:

```txt
No audio files yet.
Import audio to start building your session.
[Import Audio]
```

Empty states should feel calm, not annoying.

---

## Loading States

Loading must be visible and polished.

Use:

* Skeleton rows
* Subtle progress bars
* Inline status text
* Disabled states during processing

For audio import:

```txt
Decoding audio…
Generating waveform…
Ready
```

Do not freeze the UI silently.

---

## Error States

Errors should be clear and useful.

Bad:

```txt
Error
```

Good:

```txt
Could not decode this audio file.
Try WAV, MP3, or FLAC.
```

Error UI should not destroy the layout.

Use inline errors or toast notifications depending on severity.

---

## Motion

Motion should be subtle.

Acceptable:

* Fast hover transitions
* Panel reveal
* Menu fade/scale
* Drag preview
* Clip snap feedback

Avoid:

* Bouncy animations
* Slow transitions
* Overly playful motion
* Motion that delays workflow

Recommended duration:

```txt
80ms to 160ms
```

Creative tools must feel instant.

---

## Interaction Rules

The app should feel like software, not a website.

Support:

* Keyboard shortcuts
* Right-click context menus
* Drag and drop
* Multi-select later
* Precise pointer interactions
* Scroll and zoom timeline
* Command palette later

Mouse interactions must be accurate.

Dragging clips must feel stable.

Do not allow jittery movement.

---

## Keyboard-First Direction

Future keyboard shortcuts should be planned early.

Examples:

```txt
Space        Play / Pause
Cmd/Ctrl+S   Save
Cmd/Ctrl+Z   Undo
Cmd/Ctrl+Y   Redo
Delete       Delete selected clip
Cmd/Ctrl+D   Duplicate
Cmd/Ctrl+K   Command palette later
```

Do not block standard browser shortcuts unnecessarily.

---

## Responsive Behavior

Initial target is desktop.

Minimum supported layout width:

```txt
1280px
```

Preferred:

```txt
1440px and above
```

Do not optimize for mobile in v0.1.

For smaller screens:

* Allow horizontal scrolling in timeline
* Collapse browser panel if needed
* Keep transport usable
* Do not break layout

---

## Accessibility

The UI must remain usable.

Requirements:

* Visible focus states
* Semantic buttons
* Keyboard-accessible controls
* ARIA labels for icon-only buttons
* Sufficient text contrast
* Do not use color alone for state
* Avoid tiny click targets for important actions

Minimum important control target:

```txt
28px height
```

Prefer:

```txt
32px height
```

---

## Tailwind Guidelines

Tailwind is allowed, but do not create messy class soup.

Prefer reusable components for repeated UI patterns:

```txt
Button
IconButton
Panel
PanelHeader
PanelBody
Input
ToolbarGroup
MenuItem
TrackHeader
Clip
```

Avoid copy-pasting long class strings everywhere.

Use design tokens or CSS variables for core colors.

---

## CSS Variable Direction

Define design tokens early.

Example:

```css
:root {
  --bg-base: #0d0f14;
  --bg-panel: #12151c;
  --bg-elevated: #181c25;

  --border-subtle: rgba(255, 255, 255, 0.07);
  --border-strong: rgba(255, 255, 255, 0.14);

  --text-primary: rgba(255, 255, 255, 0.92);
  --text-secondary: rgba(255, 255, 255, 0.64);
  --text-muted: rgba(255, 255, 255, 0.42);

  --accent: #8b9cff;
  --accent-soft: rgba(139, 156, 255, 0.16);

  --danger: #ff6b7a;
  --warning: #f4b860;
  --success: #7bd88f;

  --grid-major: rgba(255, 255, 255, 0.08);
  --grid-minor: rgba(255, 255, 255, 0.035);

  --clip-bg: rgba(139, 156, 255, 0.18);
  --clip-border: rgba(139, 156, 255, 0.4);
  --waveform: rgba(220, 225, 255, 0.72);

  --radius-sm: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;

  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-6: 24px;
}
```

Colors can change later, but token names should remain stable.

---

## Component Quality Bar

Every component must pass this checklist:

```txt
Does it have proper padding?
Does it align with nearby elements?
Does it have hover state?
Does it have focus state if interactive?
Does it have disabled state if applicable?
Does it use consistent radius?
Does it use consistent typography?
Does it avoid random colors?
Does it look intentional?
Does it still look good when empty?
Does it still look good with long text?
```

If not, fix it before moving on.

---

## Anti-Garbage UI Rules

Never ship UI that has:

```txt
No padding
Unstyled buttons
Unstyled inputs
Random gray boxes
Misaligned panels
Inconsistent text sizes
Inconsistent border radius
Random shadows
Random colors
Overflowing labels
Broken hover states
Timeline rows that do not align
Tiny unreadable controls
Default browser select boxes
Clips with no visual states
Waveforms drawn poorly
```

Bad UI is considered a functional bug.

---

## DAW-Specific UI Rules

A DAW UI must respect precision.

Important:

* Timeline alignment must be exact
* Track rows must line up with clips
* Grid must line up with ruler
* Playhead must line up with audio time
* Clip bounds must match actual duration
* Drag position must match snap/grid math
* Zoom must preserve timeline position
* Scroll must feel smooth
* Mixer controls must be readable

Visual correctness matters because it affects editing accuracy.

---

## Timeline Sizing

Recommended defaults:

```txt
Top bar height:        44px to 52px
Timeline ruler height: 28px to 36px
Track height:          72px to 96px
Track header width:    180px to 240px
Browser width:         240px to 320px
Bottom panel height:   220px to 320px
```

Do not make track lanes too small in v0.1.

Readable waveform > fake density.

---

## Z-Index Discipline

Define z-index roles.

Example:

```txt
0    base content
10   clip
20   selected clip
30   playhead
40   sticky headers
50   floating panels
60   menus/popovers
70   modals
80   toast
```

Do not randomly use `z-9999`.

---

## Scroll Behavior

Timeline scroll must be clean.

Rules:

* Horizontal scroll for time
* Vertical scroll for tracks
* Track headers should stay aligned
* Ruler should stay aligned with timeline
* Avoid nested scroll chaos
* Use sticky areas carefully

Scrolling should feel like a desktop creative app.

---

## Drag and Drop

Drag states must be visually clear.

When dragging a clip:

* Show elevated clip
* Keep original track lane highlighted
* Show snap guide if snapping
* Show valid/invalid target feedback
* Keep movement smooth

Do not make dragged elements lag behind cursor.

---

## Selection

Selection must be obvious.

Selected clip should have:

* Stronger border
* Slightly brighter fill
* Optional outline/glow
* Visible resize handles

Do not use only a tiny border color change.

---

## Save Status UI

Cloud DAW needs clear save status.

Recommended states:

```txt
Saved
Saving…
Offline
Unsaved changes
Save failed
```

Keep it subtle but visible.

Do not annoy users with huge save banners.

---

## Toasts

Use toasts only for useful events:

* Project saved
* Export completed
* Upload failed
* Audio decode failed
* Network lost

Avoid toast spam.

Toast style:

```txt
Small
Polished
Dark elevated surface
Clear text
Optional action
```

---

## Modal Rules

Use modals sparingly.

Modals should be for:

* Export settings
* Project settings
* Confirm destructive action
* Share project

Do not use modals for every small action.

Modal must have:

* Padding
* Clear title
* Clear actions
* Escape to close
* Focus trap later

---

## Command Palette Future

A command palette is planned later.

It should feel like a power-user feature.

Possible actions:

```txt
Import Audio
Create Track
Export WAV
Save Project
Toggle Mixer
Toggle Browser
Set BPM
Go to Start
Split Clip
Duplicate Clip
```

Do not implement until the core DAW works.

---

## First UI Milestone

The first UI milestone is complete when the app has:

1. A polished dark app shell
2. A top transport bar
3. A browser/assets panel
4. A timeline ruler
5. Track headers aligned with track lanes
6. At least one audio clip rendered cleanly
7. A visible waveform area
8. A playhead
9. A bottom mixer/inspector panel
10. Proper spacing and padding everywhere
11. No raw browser-default controls
12. No broken alignment
13. No random dashboard styling

The app should already look like a real creative tool, even before every feature works.

---

## Final UI Rule

If the UI looks cheap, rushed, cramped, misaligned, or generic, it is not done.

The interface must feel like something users can stare at for hours while making music.

Quality is part of the feature set.
