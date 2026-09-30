import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import { getSettings, updateSettings, type Settings } from "./settings";

afterEach(() => clearMocks());

const saved: Settings = {
  input_device: "AppleUSBAudioEngine:DJI",
  recording_quality: "small",
  generate_notes_automatically: false,
};

test("settings use the Rust command names, arguments, and field names", async () => {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_settings") return saved;
    if (cmd === "update_settings") return (args as { settings: Settings }).settings;
    return null;
  });

  expect(await getSettings()).toEqual(saved);
  const next: Settings = { ...saved, input_device: null, recording_quality: "high" };
  expect(await updateSettings(next)).toEqual(next);

  expect(calls).toEqual([
    { cmd: "get_settings", args: {} },
    { cmd: "update_settings", args: { settings: next } },
  ]);
});
