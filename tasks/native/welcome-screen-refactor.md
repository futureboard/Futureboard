# Refactor Futureboard Welcome / Studio Bootstrap Flow

> **Status note:** `A G B C D = Complete, continue it!`

## Current Goals

1. Initialize **WGPU / Studio Layout** from the beginning while still on the Welcome screen.
2. Move the **Open Project** dialog/flow into the Welcome screen instead of opening separate scattered dialogs.
3. Allow naming a new project directly in Welcome.
4. Support **Continue without project / without welcome flow** into Studio window.
5. When saving a project from Studio Windowed mode, copy all referenced/imported assets into the created project folder.

## Hard Rules

* Do not break existing Studio Layout.
* Do not break existing recent projects.
* Do not block UI while initializing WGPU/audio/plugin systems.
* Do not create fake project files until user explicitly creates/saves.
* Do not copy assets on every UI edit.
* Asset copy must happen on project create/save/import commit, not during render.
* Use global theme colors only.
* No hardcoded custom colors.
* Keep Welcome compact and DAW-like.

---

## Part A — Initialize Studio Layout + WGPU During Welcome

### Current Behavior

Welcome screen appears before full Studio Layout / WGPU timeline systems are ready.

### Goal

Initialize the Studio runtime early, even while Welcome is visible.

### Meaning

* Create/prepare StudioLayout state at app startup.
* Initialize renderer/backend state early.
* Initialize WGPU timeline renderer if GPU Acceleration is selected.
* Initialize audio engine/device warmup if appropriate.
* Keep Welcome as an overlay/start surface, not a separate disconnected app mode.

### Architecture

App boot:

1. Load preferences/settings.
2. Create app state.
3. Create StudioLayout state.
4. Initialize render backend selection:

   * **GPU Acceleration** → init WGPU renderer async/early
   * **CPU Render** → init GPUI paint fallback
5. Warm audio engine/device if current behavior supports it.
6. Show Welcome screen as the active front/start route.
7. When user creates/opens/continues, switch into Studio workspace instantly.

### Important

* WGPU init should be non-blocking.
* If WGPU fails, fallback to CPU Render and log.
* Welcome should show status:

  * Renderer: GPU ready / CPU fallback
  * Audio: Ready / Not configured / Error
  * Plugin scan: Idle / Scanning / Ready later

### Debug Flags

```bash
FUTUREBOARD_BOOT_DEBUG=1
FUTUREBOARD_GPU_RENDERER_DEBUG=1
```

### Logs

* app boot start
* settings loaded
* StudioLayout initialized
* renderer mode selected
* WGPU init start/end/fail
* Welcome shown
* workspace entered

### Acceptance Criteria

* [ ] Welcome opens with StudioLayout/runtime already initialized.
* [ ] Entering Studio does not recreate entire app state unnecessarily.
* [x] WGPU can be ready before opening workspace.
* [x] If WGPU fails, app still works with CPU Render.

### Progress Notes

* 2026-06-04: Studio first paint no longer waits for native audio engine creation,
  device enumeration, or stream warm-up. Audio warm-up now runs on the background
  executor after the Studio shell is constructed. Full pre-created StudioLayout
  while still on Welcome remains unchecked.

---

## Part B — Move Open Project Dialog Into Welcome

### Goal

Open Project should be part of the Welcome flow.

### Welcome Tabs

* Welcome
* New Project
* Open Project
* Recent Projects
* Feeds
* Audio Setup

### Open Project Tab Should Include

* Project file picker button
* Recent project list
* Drag/drop project file later
* Validation status
* Missing file warning
* Supported project formats

### Supported Formats

* `.fbs` / current Futureboard project format
* `.fbproj` if planned
* `.dawproject` later as import

### Open Project Flow

1. User clicks Open Project tab.
2. User clicks Browse / Choose Project.
3. Native file picker opens.
4. User selects project.
5. Validate project file.
6. Load project.
7. Add to recent projects.
8. Switch to Studio workspace.

### Cancel Behavior

* Remain on Welcome.
* No error.

### Invalid Project Behavior

* Show inline error in Welcome.
* Do not crash.

### Do Not

* Open separate random dialog window unless native file picker is needed.
* Load project before validation.
* Block UI during load if large.

### Acceptance Criteria

* [x] Open Project flow lives in Welcome.
* [x] Recent list and file picker are accessible from Welcome.
* [x] Invalid project shows inline error.
* [x] Successful open enters Studio workspace.

---

## Part C — New Project Naming Directly In Welcome

### Goal

User can create a named project directly from Welcome.

### New Project Tab Fields

* Project Name
* Project Location
* Template
* Sample Rate
* BPM
* Time Signature
* Create Project button
* Continue without saving/project button

### Suggested UI

```text
Project Name
[ Untitled Project ]

Location
[ Documents/Futureboard Studio/Projects ] [Change...]

Template
[ Empty Project / MIDI Composer / Audio Session / Mix Template ]

Audio
Sample Rate: [48 kHz]
BPM: [120]
Time Signature: [4/4]

Buttons:
[ Create Project ] [ Continue Without Project ]
```

### Behavior

* Project name sanitizes folder/file name.
* Empty name falls back to `Untitled Project`.
* Default project path comes from preferences.
* Project folder preview:

```text
<default_project_path>/<project_name>/
```

* If folder exists:

  * Ask overwrite
  * Choose different name
  * Open existing
* Create folder only when user creates project.
* Create project file inside folder.
* Copy default template/assets if template requires it.
* Enter Studio workspace after create.

### Project Folder Structure

```text
Futureboard Studio/Projects/My Song/
├─ My Song.fbs
├─ Assets/
│  ├─ Audio/
│  ├─ MIDI/
│  ├─ Samples/
│  ├─ Presets/
│  └─ Plugins/
├─ Cache/
│  ├─ Peaks/
│  ├─ Waveforms/
│  └─ Analysis/
├─ Bounces/
├─ Exports/
└─ Backups/
```

### Acceptance Criteria

* [x] User can type project name in Welcome.
* [x] Create Project creates project folder.
* [x] Project file is created.
* [x] Studio opens with named project.
* [x] Recent projects update.

---

## Part D — Continue Without Project / Without Welcome Screen

### Goal

Support entering Studio without creating a project.

### Button Text

Use one of:

* `Continue Without Project`
* `Open Empty Workspace`

### Behavior

* Creates unsaved in-memory project/workspace.
* Does not create folder.
* Does not copy assets yet.
* Title bar shows:

```text
Untitled Project — Unsaved
```

* User can work normally.
* First Save / Save As prompts for project folder/name.
* After first save, copy referenced assets into project folder.

### Preference

Support this setting:

```text
Show Welcome on startup: On/Off
```

If disabled:

* App boots directly into Studio empty workspace.
* StudioLayout/WGPU already initialized.
* User can still open Welcome from `File > Welcome / Start Page`.

### Acceptance Criteria

* [x] Continue Without Project enters Studio immediately.
* [x] No folder is created until Save.
* [x] Save As asks for project name/location.
* [x] Welcome can be disabled on startup.

---

## Part E — Save Project From Studio Windowed Mode And Copy All Assets

### Goal

When saving a project, all referenced/imported assets should be copied into the project folder.

### Scope

Copy:

* Audio files
* MIDI files if imported externally
* Samples
* Presets used by project if local files
* Project templates/assets
* Plugin preset files if explicitly project-owned

Do **not** copy by default:

* Plugin binaries

Marketplace assets later can be referenced by license/cache rules.

### Hard Rules

* Do not copy plugin binaries by default.
* Do not duplicate assets repeatedly.
* Do not copy missing files silently.
* Do not block UI for large copy operations.
* Preserve original file extension.
* Handle filename collisions.
* Use content hash or stable asset id to avoid duplicates.
* Project should reference local copied asset path after copy.
* If asset copy fails, show clear error and keep original reference until resolved.

### Project Asset Copy Modes

1. Copy on Import
2. Copy on Save
3. Reference Original
4. Ask Each Time

### Initial Recommended Behavior

* Copy on Save for unsaved workspace.
* Copy on Import for created project if enabled.
* Always make project portable by default.

### Asset Folder Layout

```text
Assets/Audio/
Assets/MIDI/
Assets/Samples/
Assets/Presets/
Assets/Templates/
Assets/Other/
```

### Asset Manifest

Add or update project media pool:

```rust
pub struct ProjectAsset {
    pub id: String,
    pub original_path: Option<PathBuf>,
    pub project_relative_path: PathBuf,
    pub kind: AssetKind,
    pub hash: Option<String>,
    pub size_bytes: Option<u64>,
    pub copied_at: Option<String>,
    pub missing: bool,
}
```

### AssetKind

```rust
pub enum AssetKind {
    Audio,
    MIDI,
    Sample,
    Preset,
    Template,
    Other,
}
```

### Save Flow

1. User presses Save.
2. If project has no folder:

   * Show Save Project dialog / Welcome-style Save As flow.
3. Create project folder structure.
4. Scan project references for external assets.
5. Build copy plan.
6. Show progress if assets are large.
7. Copy assets to project folder.
8. Update project references to relative paths.
9. Write project file atomically:

   * Write temp file
   * fsync/flush if possible
   * Rename temp to final
10. Add to recent projects.
11. Clear dirty state.

### Copy Plan

Each item should include:

* Source path
* Destination path
* Kind
* Collision strategy
* Already copied / skip
* Missing / error

### Collision Handling

* If same content hash: reuse existing.
* If same name but different content:

```text
filename-1.wav
filename-2.wav
```

Or use hash prefix folder.

### Atomic Save

* Never corrupt existing project file.
* Backup old project file if desired.
* Write `.tmp`.
* Rename.

### Progress UI

Show:

* `Copying assets…`
* Current file
* Bytes copied
* Cancel option if feasible

If canceled:

* Do not partially update project references.

### Acceptance Criteria

* [x] Unsaved workspace can Save As into project folder.
* [x] External audio file is copied into `Assets/Audio`.
* [x] Project references copied relative path.
* [x] Save/load works after moving project folder.
* [x] Duplicate asset is not copied repeatedly.
* [x] Missing asset shows error.
* [x] Large copy does not freeze UI.

---

## Part F — Default Project Path

### Requirement

Use existing/default settings:

* Default Project Path shown in Welcome.
* New Project uses it.
* Save As defaults to it.
* Continue Without Project save defaults to it.

### Platform Defaults

#### Windows

```text
Documents/Futureboard Studio/Projects
```

#### macOS

```text
~/Music/Futureboard Studio/Projects
```

Or current chosen convention.

#### Linux

```text
~/Music/Futureboard Studio/Projects
```

Or:

```text
~/Documents/Futureboard Studio/Projects
```

### Acceptance Criteria

* [x] Default path is used consistently.
* [x] User can change it.
* [x] Path persists.

---

## Part G — Welcome / Studio State Machine

### App Route

```rust
pub enum StudioRoute {
    Welcome,
    StudioWorkspace,
}
```

### Project State

```rust
pub enum ProjectState {
    NoProject,
    UnsavedWorkspace,
    SavedProject { path: PathBuf },
    Loading,
    Error(String),
}
```

### Flow

#### Startup

* If `show_welcome = true`:

  * `route = Welcome`
* Else:

  * `route = StudioWorkspace`
  * `project_state = UnsavedWorkspace`

#### Create Project

```text
Welcome/NewProject
→ create folder/project
→ route = StudioWorkspace + SavedProject
```

#### Open Project

```text
Welcome/OpenProject
→ load file
→ route = StudioWorkspace + SavedProject
```

#### Continue Without Project

```text
Welcome
→ route = StudioWorkspace + UnsavedWorkspace
```

#### Close Project

```text
StudioWorkspace
→ if dirty ask save
→ route = Welcome or UnsavedWorkspace depending command
```

#### Quit

```text
→ if dirty ask save
→ shutdown
```

### Acceptance Criteria

* [x] No ambiguous “Welcome but also loaded project” state.
* [x] Title bar reflects state correctly.
* [x] Save/Save As behavior is correct.

---

## Part H — UX Details

### Welcome

* New Project should be primary action.
* Open Project should be visible but not scattered.
* Recent projects should show default path and missing status.
* Feeds tab remains.
* Audio Setup remains.

### Studio Window

If unsaved:

```text
Untitled Project — Unsaved
```

If saved:

```text
<Project Name> — Saved
```

### Save Actions

* Save button disabled only if clean.
* Save As always available.

### Dialog Copy

Use:

* `Create Project`
* `Continue Without Project`
* `Save Project As…`
* `Copying Assets…`
* `Some assets could not be copied`

### Progress Notes

* 2026-06-04: Fixed detached Mixer and floating MIDI Editor window lifecycle so
  titlebar close callbacks clear StudioLayout window handles without re-entering
  the same WindowHandle and removing the window twice.

---

## Part I — Debug Flags

Add:

```bash
FUTUREBOARD_BOOT_DEBUG=1
FUTUREBOARD_WELCOME_DEBUG=1
FUTUREBOARD_PROJECT_SAVE_DEBUG=1
FUTUREBOARD_ASSET_COPY_DEBUG=1
FUTUREBOARD_GPU_RENDERER_DEBUG=1
```

### Logs

* Startup route
* WGPU init status
* Project name typed
* Default project path
* Create project folder
* Open project path
* Save/save-as path
* Asset copy plan
* Asset copied/skipped/failed
* Project file written
* Recent project updated

---

## Part J — Manual Tests

### Startup

1. Launch app.
2. Welcome appears.
3. StudioLayout/WGPU init logs show early init.
4. No UI freeze.

### New Project

1. Type project name: `meow`.
2. Confirm folder preview.
3. Click Create Project.
4. Project folder is created.
5. Studio opens.
6. Title bar shows `meow`.

### Continue Without Project

1. Launch Welcome.
2. Click Continue Without Project.
3. Studio opens unsaved.
4. No project folder created.
5. Add/import audio.
6. Press Save.
7. Save As asks name/location.
8. Project folder created.
9. Assets copied.

### Open Project

1. Welcome → Open Project.
2. Select existing `.fbs`.
3. Project loads.
4. Recent list updates.

### Asset Copy

1. Unsaved workspace imports external WAV.
2. Save project.
3. WAV copied to `Assets/Audio`.
4. Project file references relative path.
5. Move project folder.
6. Reopen project.
7. Audio still resolves.

### Failure

1. Add missing asset reference.
2. Save.
3. Error shown.
4. No crash.

### Settings

1. Disable Show Welcome on startup.
2. Relaunch.
3. Opens directly to Studio unsaved.
4. Welcome still accessible from menu.

### Build

```bash
cargo check -p sphere_ui_components
cargo check --manifest-path apps/native/Cargo.toml
cargo clippy -p sphere_ui_components -- -D warnings
```

### Progress Notes

* 2026-06-04: Fixed CI warning cleanup for non-Windows builds: native message
  box GPUI imports are now scoped to the Windows-only open path, and plugin
  editor window dimensions are marked intentionally unused for non-Win32
  backends.

---

## Final Acceptance Criteria

* [ ] WGPU / Studio Layout initializes at app startup even while Welcome is shown.
* [x] Welcome owns New Project / Open Project / Continue Without Project flows.
* [x] User can name project directly in Welcome.
* [x] Default project path is used consistently.
* [x] Continue Without Project creates unsaved workspace only.
* [ ] Save/Save As from Studio creates project folder and copies all referenced assets.
* [x] Project assets are stored relative to the project folder.
* [x] Asset copy is deduped and error-safe.
* [x] Existing Studio workflow remains stable.
