import "./dawBridge.types";
import { electronPlatform } from "./platform.electron";
import { webPlatform } from "./platform.web";
import type { Platform } from "./platform.types";

/**
 * Platform singleton. Detected once at module load by sniffing for the
 * Electron preload bridge (`window.dawElectron`). UI code should import
 * from here and never branch on `window.*` directly.
 */
export const platform: Platform =
  typeof window !== "undefined" && window.dawElectron
    ? electronPlatform
    : webPlatform;

export type { Platform } from "./platform.types";
export type {
  DialogAdapter,
  FileSystemAdapter,
  MessageBoxKind,
  MessageBoxOptions,
  MessageBoxResult,
  PlatformCapabilities,
  PlatformKind,
  ProjectStorageAdapter,
  SaveProjectOptions,
  SaveProjectResult,
  WindowAdapter,
} from "./platform.types";
