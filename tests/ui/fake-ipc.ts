import type { Page } from "@playwright/test";

// Fake Rust commands, keyed by command name. Values must be JSON.
export type FakeCommands = Record<string, unknown>;

export const notesFolder = {
  path: "/Users/someone/Documents/Anchovy",
  display: "~/Documents/Anchovy",
  exists: true,
  obsidian_vault: false,
};

export const defaultCommands: FakeCommands = {
  app_info: { name: "Anchovy", version: "0.1.0" },
  list_models: { memory_bytes: 16 * 1024 ** 3, models: [] },
  list_recordings: [],
  read_note: { source: null, inputs: [], sections: [] },
  show_in_finder: null,
  move_to_trash: null,
  list_input_devices: [{ uid: "BuiltIn", name: "MacBook Air Microphone", is_default: true }],
  recording_sources: { microphone: "MacBook Air Microphone", computer_audio: null },
  // First launch done, everything allowed.
  setup_status: {
    notes_folder: notesFolder,
    default_folder: notesFolder,
    microphone: "allowed",
    computer_audio: "allowed",
    finished: true,
    can_record: true,
  },
  check_computer_audio: "allowed",
  // Event listeners: the interface subscribes to download progress.
  "plugin:event|listen": 0,
  "plugin:event|unlisten": null,
};

// Installs a stand-in for Tauri's IPC bridge before the page loads. Events
// the interface listens to can be sent with `emitFakeEvent`.
export async function installFakeIpc(page: Page, commands: FakeCommands = defaultCommands) {
  await page.addInitScript((responses: FakeCommands) => {
    let nextId = 0;
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners: { event: string; handler: number }[] = [];
    Object.assign(window, {
      __emitFakeEvent: (event: string, payload: unknown) => {
        for (const listener of listeners.filter((l) => l.event === event)) {
          callbacks.get(listener.handler)?.({ event, id: listener.handler, payload });
        }
      },
      __TAURI_INTERNALS__: {
        invoke: async (cmd: string, args?: { event?: string; handler?: number }) => {
          if (cmd === "plugin:event|listen" && args?.event && args.handler !== undefined) {
            listeners.push({ event: args.event, handler: args.handler });
            return args.handler;
          }
          if (cmd in responses) return responses[cmd];
          throw new Error(`No fake for command ${cmd}`);
        },
        transformCallback: (callback: (payload: unknown) => void) => {
          const id = nextId++;
          callbacks.set(id, callback);
          return id;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      },
    });
  }, commands);
}

export async function emitFakeEvent(page: Page, event: string, payload: unknown) {
  await page.evaluate(
    ([name, data]) =>
      (window as unknown as { __emitFakeEvent: (e: unknown, p: unknown) => void }).__emitFakeEvent(
        name,
        data,
      ),
    [event, payload] as const,
  );
}
