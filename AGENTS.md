## Project Overview

This project is a cloud-first Digital Audio Workstation built with React and the Web Audio API.

The goal is to create a browser-based DAW that can run on a web server, allowing users to import audio files, arrange clips on a timeline, edit tracks, mix audio, save projects to the cloud, and eventually support MIDI, automation, collaboration, and offline rendering.

The initial implementation focuses on a React frontend with a Web Audio playback engine. Native audio backends, desktop wrappers, or plugin hosting may be added later, but the first milestone must work entirely in the browser.

---

## Core Direction

The project should prioritize:

- Browser-first architecture
- React + TypeScript frontend
- Web Audio API for playback and mixing
- AudioWorklet where needed for advanced processing
- Canvas or WebGL for timeline and waveform rendering
- Cloud project storage
- Object storage for audio files
- Clean separation between UI state and audio engine state
- Incremental development through small, working milestones

Avoid designing the system as a native DAW first. This is a web DAW first.

---

## Recommended Tech Stack

### Frontend

- React
- TypeScript
- Vite
- Zustand or Jotai for state management
- Tailwind CSS for UI
- Canvas, OffscreenCanvas, or WebGL for heavy rendering
- Web Workers for waveform generation and background processing
- Web Audio API
- AudioWorklet for real-time DSP when necessary
- IndexedDB or OPFS for local project cache

### Backend

- Node.js or Bun
- PostgreSQL for project metadata
- S3, R2, or MinIO for audio file storage
- REST API first, WebSocket later for collaboration
- Presigned upload URLs for large audio files

### Optional Later

- WebCodecs
- WASM DSP modules
- CRDT-based collaboration
- OfflineAudioContext rendering
- MIDI editor
- Web MIDI API
- Plugin-like internal device system

---

## Repository Structure

Prefer a monorepo layout.

```txt
mochi-daw/
├─ apps/
│  ├─ web/
│  └─ server/
│
├─ packages/
│  ├─ daw-core/
│  ├─ daw-engine/
│  ├─ daw-project/
│  ├─ daw-waveform/
│  ├─ daw-ui/
│  └─ daw-shared/
│
├─ docs/
├─ AGENTS.md
├─ package.json
└─ README.md
````

### Suggested Package Responsibilities

```txt
daw-core
- Shared DAW models
- Timeline math
- Track and clip utilities
- Transport state helpers

daw-engine
- Web Audio engine
- Audio graph management
- Playback scheduler
- Mixer routing
- Offline render logic

daw-project
- Project serializer
- Project loader
- Cloud project sync
- Version migration

daw-waveform
- Peak generation
- Waveform cache
- Worker-based audio analysis

daw-ui
- Shared React components
- Timeline components
- Mixer components
- Transport UI

daw-shared
- Shared API types
- Validation schemas
- Common utilities
```

---

## Development Principles

### 1. Keep Audio Engine Separate From React

React components must not directly own playback logic.

Do not schedule audio inside UI components.

Use a dedicated engine layer:

```txt
React UI
→ DAW Store
→ Audio Engine
→ Web Audio API
```

React should send commands such as:

```txt
play()
pause()
stop()
seek(time)
setTrackVolume(trackId, value)
moveClip(clipId, startTime)
```

The audio engine should handle actual Web Audio scheduling internally.

---

### 2. Use AudioContext Time for Playback

Do not use `Date.now()` as the source of truth for playback timing.

Use:

```ts
audioContext.currentTime
```

Transport timing should track both:

```txt
AudioContext Time
Project Timeline Time
```

Recommended model:

```ts
transportStartAudioTime = audioContext.currentTime;
transportStartProjectTime = currentPlayheadTime;

projectTime =
  transportStartProjectTime +
  (audioContext.currentTime - transportStartAudioTime);
```

---

### 3. Use Seconds as the Internal Timeline Unit

For the first versions, use seconds as the main timeline unit.

Avoid introducing bars, beats, ticks, PPQ, or tempo maps too early.

Initial timeline values:

```ts
startTime: number;
duration: number;
offset: number;
```

Later, BPM grid and musical time can be layered on top.

---

### 4. Build the MVP Before Advanced DAW Features

Do not start with MIDI, plugins, automation, or collaboration.

The first target is:

```txt
Import audio
→ Decode audio
→ Generate waveform
→ Place clip on timeline
→ Play from timeline
→ Move playhead correctly
→ Adjust track volume
→ Save project locally
```

This must be stable before adding more features.

---

## MVP Scope

### Version 0.1

Required features:

* Create a project
* Import WAV or MP3
* Decode audio with Web Audio
* Generate waveform peaks
* Create audio tracks
* Add clips to tracks
* Render clips on a timeline
* Play, pause, stop
* Seek timeline
* Show moving playhead
* Track volume
* Master volume
* Local project save

Out of scope for v0.1:

* MIDI
* VST/AU/LV2 plugins
* Real-time collaboration
* Advanced automation
* Time stretching
* Pitch shifting
* Full mastering tools
* Multi-user editing
* Native audio device control
* ASIO/CoreAudio/JACK backend

---

## Data Model

Use a simple project format first.

```ts
export type DawProject = {
  id: string;
  name: string;
  version: number;
  sampleRate: number;
  bpm: number;
  tracks: DawTrack[];
  files: DawFile[];
};

export type DawTrack = {
  id: string;
  name: string;
  type: "audio";
  volume: number;
  pan: number;
  muted: boolean;
  solo: boolean;
  clips: DawClip[];
};

export type DawClip = {
  id: string;
  name: string;
  fileId: string;
  trackId: string;
  startTime: number;
  offset: number;
  duration: number;
  gain: number;
};

export type DawFile = {
  id: string;
  name: string;
  mimeType: string;
  duration: number;
  sampleRate: number;
  channels: number;
  storageKey?: string;
  localObjectUrl?: string;
};
```

---

## Project File Format

The project should be serializable as JSON.

Example:

```json
{
  "id": "project_001",
  "name": "Mochi Beat",
  "version": 1,
  "sampleRate": 48000,
  "bpm": 120,
  "tracks": [
    {
      "id": "track_001",
      "name": "Drums",
      "type": "audio",
      "volume": 0.9,
      "pan": 0,
      "muted": false,
      "solo": false,
      "clips": [
        {
          "id": "clip_001",
          "name": "drum-loop.wav",
          "fileId": "file_001",
          "trackId": "track_001",
          "startTime": 0,
          "offset": 0,
          "duration": 8,
          "gain": 1
        }
      ]
    }
  ],
  "files": [
    {
      "id": "file_001",
      "name": "drum-loop.wav",
      "mimeType": "audio/wav",
      "duration": 8,
      "sampleRate": 48000,
      "channels": 2
    }
  ]
}
```

---

## Audio Engine Guidelines

The Web Audio engine should expose a clean class-based or service-based API.

Example responsibilities:

```txt
AudioEngine
- Own AudioContext
- Own master output
- Load and cache AudioBuffers
- Start and stop playback
- Connect mixer nodes
- Manage track gain and pan nodes

Transport
- Track play state
- Track current project time
- Handle play, pause, stop, seek

ClipScheduler
- Schedule AudioBufferSourceNode instances
- Handle clip offsets
- Handle clips that begin before or after the playhead

Mixer
- Track volume
- Pan
- Mute
- Solo
- Master gain
```

Do not create a new AudioContext for every playback action.

There should usually be one main AudioContext per app session.

---

## Scheduling Rules

When playback starts:

1. Resume the AudioContext if needed.
2. Store the current AudioContext time.
3. Store the current project playhead time.
4. Find all clips that overlap the playback window.
5. Schedule clips using `AudioBufferSourceNode.start()`.
6. Update the UI playhead based on AudioContext time.

Clip scheduling must account for:

```txt
clip.startTime
clip.offset
clip.duration
current playhead time
```

If the playhead starts inside a clip, start playback from the correct clip offset.

---

## Waveform Guidelines

Waveform rendering must not block the main UI thread for large files.

Preferred flow:

```txt
User imports file
→ Decode audio
→ Send buffer data to worker
→ Generate peaks
→ Cache peaks
→ Render waveform using Canvas
```

Use peak arrays instead of drawing raw samples.

Suggested format:

```ts
export type WaveformPeaks = {
  samplesPerPeak: number;
  channelCount: number;
  peaks: Float32Array;
};
```

Render waveform with Canvas, not thousands of DOM elements.

---

## UI Guidelines

The UI should feel like a modern DAW.

Suggested layout:

```txt
┌────────────────────────────────────────────┐
│ Transport / Toolbar / BPM / Project Name   │
├───────────────┬────────────────────────────┤
│ Track Headers │ Timeline Arrangement       │
│               │ Clips and Waveforms        │
├───────────────┴────────────────────────────┤
│ Mixer / Inspector / Browser Panel          │
└────────────────────────────────────────────┘
```

Core components:

```txt
AppShell
TransportBar
Timeline
TimelineRuler
TrackList
TrackHeader
TrackLane
AudioClip
WaveformCanvas
Playhead
MixerPanel
InspectorPanel
BrowserPanel
```

Keep timeline rendering performant.

Use React for structure and interaction, Canvas for dense visual data.

---

## State Management

Use a central store for project state.

Recommended store sections:

```txt
project
tracks
clips
files
selection
timeline
transport
ui
```

Avoid duplicating state between the UI store and audio engine.

The project state is the source of truth for arrangement data.

The audio engine may keep runtime-only state such as:

```txt
AudioContext
AudioBuffer cache
GainNode map
PanNode map
Currently scheduled sources
Transport start audio time
```

---

## Undo / Redo

Undo and redo should be designed early.

Prefer command-style actions:

```txt
MoveClipCommand
ResizeClipCommand
SplitClipCommand
DeleteClipCommand
AddTrackCommand
DeleteTrackCommand
SetTrackVolumeCommand
```

Each command should support:

```ts
execute()
undo()
redo()
```

For v0.1, a simple history stack is acceptable.

---

## Cloud Architecture

Audio files should not be stored directly in the database.

Use object storage.

Recommended cloud layout:

```txt
projects
tracks
clips
files
users

object-storage/
├─ audio/original/
├─ audio/proxy/
├─ peaks/
└─ exports/
```

Recommended flow for uploads:

```txt
Client requests upload URL
→ Server creates presigned URL
→ Client uploads file directly to object storage
→ Server saves metadata
→ Client links file to project
```

---

## API Guidelines

Initial REST endpoints:

```txt
POST   /api/projects
GET    /api/projects/:projectId
PUT    /api/projects/:projectId
DELETE /api/projects/:projectId

POST   /api/projects/:projectId/files
GET    /api/projects/:projectId/files/:fileId

POST   /api/projects/:projectId/save
POST   /api/projects/:projectId/export
```

Later WebSocket events:

```txt
project.updated
track.created
track.updated
clip.created
clip.updated
clip.deleted
transport.updated
presence.updated
```

---

## Local Storage Guidelines

Use IndexedDB or OPFS for local cache.

Local cache may store:

```txt
Project JSON
Imported audio files
Decoded metadata
Waveform peaks
Autosave snapshots
```

Do not rely only on memory for large audio projects.

---

## Performance Guidelines

Important browser DAW constraints:

* `decodeAudioData()` loads the full audio file into memory.
* Long audio files may consume significant RAM.
* Large waveforms must be rendered with Canvas.
* Scheduling must use Web Audio timing.
* React state updates should not happen every audio frame.
* Playhead UI can update with `requestAnimationFrame`.
* Avoid rerendering all tracks on every playhead tick.

For playhead movement, prefer isolated rendering.

Do not cause the entire timeline to rerender every frame.

---

## Testing Guidelines

Test the following early:

* Import WAV
* Import MP3
* Decode failure handling
* Playback from start
* Playback from middle of clip
* Playback when clip starts after playhead
* Multiple clips on one track
* Multiple tracks
* Volume changes during playback
* Stop and restart playback
* Seek while playing
* Remove clip while playing
* Browser refresh and project restore

---

## Browser Compatibility

Initial target:

```txt
Chrome / Chromium based browsers
```

Later support:

```txt
Firefox
Safari
Mobile browsers
```

Do not optimize for mobile in the first MVP unless explicitly required.

---

## Naming Conventions

Use clear DAW terms.

Preferred names:

```txt
Project
Track
Clip
File
Region
Transport
Timeline
Arrangement
Mixer
Bus
Master
Playhead
Waveform
```

Avoid vague names like:

```txt
Item
Thing
Data
NodeObject
AudioStuff
```

---

## Coding Style

Use TypeScript strictly.

Prefer:

```ts
type TrackId = string;
type ClipId = string;
type FileId = string;
```

Use explicit types for DAW data models.

Avoid `any` unless absolutely necessary.

Keep audio engine code deterministic and easy to debug.

---

## Error Handling

Handle these cases gracefully:

* Browser blocks AudioContext before user interaction
* Unsupported audio file format
* Failed audio decoding
* Missing audio file
* Broken project JSON
* Failed cloud upload
* Network loss during save
* AudioContext suspended
* Out-of-memory during large file import

Show user-friendly errors in the UI.

Do not crash the app on bad files.

---

## Security Notes

For cloud usage:

* Validate uploaded file types
* Limit file size
* Use signed URLs
* Do not expose storage credentials to the browser
* Validate project JSON on the server
* Authenticate all project operations
* Ensure users can only access their own projects

---

## Future Roadmap

### v0.1

* Audio import
* Timeline
* Waveform
* Playback
* Track volume
* Local save

### v0.2

* Cloud save/load
* File upload
* Autosave
* Clip move, trim, split
* Undo/redo

### v0.3

* Mixer
* Pan
* Mute/solo
* Metering
* Export WAV with OfflineAudioContext

### v0.4

* Automation lanes
* Clip gain
* Fade in/out
* Snap grid
* Loop region

### v0.5

* MIDI editor
* Web MIDI input
* Built-in synth
* Sampler
* Drum rack

### v0.6

* Collaboration
* Presence
* Project sharing
* Version history
* Cloud rendering

---

## Things To Avoid

Do not:

* Put audio scheduling inside React components
* Create many AudioContexts
* Use DOM elements for every waveform sample
* Store large audio files in PostgreSQL
* Use UI frame timing as audio timing
* Add advanced features before stable playback
* Build plugin hosting before the core DAW works
* Overcomplicate v0.1 with native architecture
* Treat this like Electron-first software
* Assume browser audio behaves like native low-latency audio

---

## First Milestone Definition

The first milestone is complete when the app can:

1. Create a new project
2. Import an audio file
3. Decode the file
4. Generate a visible waveform
5. Add the audio as a clip on a track
6. Move the clip on the timeline
7. Press play and hear the clip at the correct time
8. Show a playhead that stays in sync with audio
9. Change track volume
10. Save and reload the project locally

Once this works, the project can be considered a real browser DAW prototype.

---

## Product Vision

The long-term goal is to build a lightweight, cloud-native DAW that feels fast, modern, and accessible.

The app should eventually support:

* Browser-based music production
* Cloud project storage
* Collaboration
* Audio editing
* MIDI editing
* Built-in instruments
* Built-in effects
* Offline rendering
* Shareable sessions
* Version history

The first version should be simple, stable, and fun to use.

Make the core playback experience solid before making the app huge.
