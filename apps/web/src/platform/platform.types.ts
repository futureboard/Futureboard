import type { DawProject } from "../types/daw";

export type PlatformKind = "web" | "electron" | "sphere-native";

export type PlatformCapabilities = {
  kind: PlatformKind;
  /** Real filesystem access (open/save dialogs, reveal in OS file manager). */
  filesystem: boolean;
  /** Persistent local project storage beyond browser localStorage. */
  persistentLocalProjects: boolean;
  /** Native OS open/save/message dialogs available. */
  nativeDialogs: boolean;
  /** Native window controls (minimize, maximize, close). */
  nativeWindowControls: boolean;
  /** Native (non Web Audio) DSP/audio engine available. */
  nativeAudioEngine: boolean;
  /** Native plugin hosting (VST/AU/LV2) available. */
  nativePlugins: boolean;
  /** Web Audio API available. */
  webAudio: boolean;
  /** Cloud project sync available. */
  cloudSync: boolean;
  /** OS-level file paths are meaningful to callers (e.g. reveal). */
  osFilePaths: boolean;
};

/**
 * Filesystem-style operations. On web these may be stubs or hidden
 * `<input type="file">` plumbing; on Electron they bridge to Node.
 */
export interface FileSystemAdapter {
  /** Prompt the user to pick one or more audio files. Returns real `File` objects. */
  pickAudioFiles(): Promise<File[]>;
  /** Read an audio asset from a trusted native path. Electron only; web returns null. */
  readAudioFile(path: string): Promise<File | null>;
  /** Reveal a file in the OS file manager. No-op or throws on web. */
  revealInFileManager(path: string): Promise<void>;
}

export type SaveProjectOptions = {
  saveAs?: boolean;
};

export type SaveProjectResult = {
  path?: string;
};

export interface ProjectStorageAdapter {
  /** Persist a project. On web → localStorage. On Electron → file. Returns metadata or null if cancelled. */
  saveProject(
    project: DawProject,
    opts?: SaveProjectOptions,
  ): Promise<SaveProjectResult | null>;
  /** Load a project. On web → localStorage. On Electron → file picker. */
  openProject(): Promise<DawProject | null>;
}

export type MessageBoxKind = "none" | "info" | "error" | "question" | "warning";

export type MessageBoxOptions = {
  type?: MessageBoxKind;
  title?: string;
  message: string;
  detail?: string;
  buttons?: string[];
};

export interface DialogAdapter {
  showMessageBox(opts: MessageBoxOptions): Promise<void>;
  showErrorBox(title: string, message: string): Promise<void>;
}

export interface WindowAdapter {
  minimize(): void;
  toggleMaximize(): void;
  close(): void;
}

export interface Platform {
  kind: PlatformKind;
  capabilities: PlatformCapabilities;
  fileSystem: FileSystemAdapter;
  projectStorage: ProjectStorageAdapter;
  dialog: DialogAdapter;
  window: WindowAdapter;
}
