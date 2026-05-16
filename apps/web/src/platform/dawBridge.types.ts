/**
 * Type declarations for the `window.dawElectron` bridge exposed by the
 * Electron preload (`apps/electron/src/preload.ts`).
 *
 * Kept manually in sync with the preload surface. Importing this file
 * augments the global `Window` interface so platform adapters can
 * detect and call the bridge without `any`.
 */

export type DawBridgePlatform =
  | "aix"
  | "darwin"
  | "freebsd"
  | "linux"
  | "openbsd"
  | "sunos"
  | "win32"
  | (string & {});

export type DawBridgePickedAudioFile = {
  name: string;
  mimeType: string;
  bytes: ArrayBuffer;
  path: string;
  size: number;
  lastModified: number;
};

export type DawBridgeAudioFileStat = Omit<DawBridgePickedAudioFile, "bytes">;

export type DawBridgeMessageBoxKind =
  | "none"
  | "info"
  | "error"
  | "question"
  | "warning";

export type DawBridgeMessageBoxOptions = {
  type?: DawBridgeMessageBoxKind;
  title?: string;
  message: string;
  detail?: string;
  buttons?: string[];
};

export type DawBridgeMessageBoxResult = {
  response: number;
};

export type DawBridgeSaveDialogResult = {
  canceled: boolean;
  path?: string;
};

export type DawBridgeOpenDialogResult = {
  canceled: boolean;
  path?: string;
};

export interface DawBridgeFs {
  pickAudioFiles(): Promise<DawBridgePickedAudioFile[]>;
  readAudioFile(path: string): Promise<DawBridgePickedAudioFile | null>;
  statAudioFile(path: string): Promise<DawBridgeAudioFileStat | null>;
  revealInFileManager(path: string): Promise<void>;
}

export type DawBridgeFolderCreateOptions = {
  name: string;
  location: string;
};

export type DawBridgeFolderCreateResult = {
  projectRoot: string;
  projectFilePath: string;
};

export type DawBridgeFolderImportResult = {
  relativePath: string;
  absolutePath: string;
  name: string;
  size: number;
  lastModified: number;
};

export type DawBridgeBrowseFolderResult = {
  canceled: boolean;
  folderPath?: string;
};

export interface DawBridgeProject {
  showSaveDialog(suggestedName?: string): Promise<DawBridgeSaveDialogResult>;
  showOpenDialog(): Promise<DawBridgeOpenDialogResult>;
  read(path: string): Promise<string | null>;
  write(path: string, contents: string): Promise<boolean>;
  // Folder project operations
  browseFolderLocation(): Promise<DawBridgeBrowseFolderResult>;
  createFolderProject(options: DawBridgeFolderCreateOptions): Promise<DawBridgeFolderCreateResult | null>;
  saveFolderProject(projectRoot: string, contents: string): Promise<boolean>;
  openFolderFile(filePath: string): Promise<string | null>;
  importAudioToFolder(projectRoot: string, sourcePath: string): Promise<DawBridgeFolderImportResult | null>;
}

export type DawBridgeGpuFeatureStatus = {
  hardwareAccelerationEnabled: boolean;
  features: Record<string, string>;
  gpuDescription: string | null;
  electronVersion: string;
  chromeVersion: string;
};

export interface DawBridgeSys {
  getGpuInfo(): Promise<DawBridgeGpuFeatureStatus>;
}

export interface DawBridgeDialog {
  showMessageBox(
    options: DawBridgeMessageBoxOptions,
  ): Promise<DawBridgeMessageBoxResult>;
  showErrorBox(title: string, message: string): Promise<void>;
}

export interface DawBridgeWindow {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
}

export interface DawElectronBridge {
  /** Legacy/back-compat surface preserved for existing renderer consumers. */
  platform: DawBridgePlatform;
  frameless: boolean;
  transparentWindow: boolean;
  windowControlsOverlayEnabled: boolean;

  fs: DawBridgeFs;
  project: DawBridgeProject;
  dialog: DawBridgeDialog;
  window: DawBridgeWindow;
  sys: DawBridgeSys;
}

declare global {
  interface Window {
    dawElectron?: DawElectronBridge;
  }
}

export {};
