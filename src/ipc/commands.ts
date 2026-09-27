import { invoke } from "@tauri-apps/api/core";

// Mirrors `AppInfo` in src-tauri/src/lib.rs.
export type AppInfo = {
  name: string;
  version: string;
};

export function appInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}
