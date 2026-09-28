import type { Page } from "@playwright/test";

// Fake Rust commands, keyed by command name. Values must be JSON.
export type FakeCommands = Record<string, unknown>;

export const defaultCommands: FakeCommands = {
  app_info: { name: "Anchovy", version: "0.1.0" },
  list_models: { memory_bytes: 16 * 1024 ** 3, models: [] },
  list_recordings: [],
  read_note: { source: null, inputs: [], sections: [] },
  show_in_finder: null,
  move_to_trash: null,
  // Event listeners: the interface subscribes to download progress.
  "plugin:event|listen": 0,
  "plugin:event|unlisten": null,
};

// Installs a stand-in for Tauri's IPC bridge before the page loads.
export async function installFakeIpc(page: Page, commands: FakeCommands = defaultCommands) {
  await page.addInitScript((responses: FakeCommands) => {
    let nextId = 0;
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        invoke: async (cmd: string) => {
          if (cmd in responses) return responses[cmd];
          throw new Error(`No fake for command ${cmd}`);
        },
        transformCallback: () => nextId++,
        unregisterCallback: () => {},
        metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      },
    });
  }, commands);
}
