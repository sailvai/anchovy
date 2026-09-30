import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { afterEach, expect, test } from "vitest";
import {
  listInputDevices,
  onRecordingProgress,
  recordingSources,
  startRecording,
  stopRecording,
  type RecordingProgress,
} from "./recording";

afterEach(() => clearMocks());

test("commands use the Rust names; Rust reads the saved device itself", async () => {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_input_devices") {
      return [{ uid: "BuiltIn", name: "MacBook Air Microphone", is_default: true }];
    }
    return null;
  });

  const devices = await listInputDevices();
  await recordingSources();
  await startRecording();
  await stopRecording();

  expect(devices[0].name).toBe("MacBook Air Microphone");
  expect(calls).toEqual([
    { cmd: "list_input_devices", args: {} },
    { cmd: "recording_sources", args: {} },
    { cmd: "start_recording", args: {} },
    { cmd: "stop_recording", args: {} },
  ]);
});

test("progress arrives from the recording-progress event", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  const seen: RecordingProgress[] = [];
  const unlisten = await onRecordingProgress((progress) => seen.push(progress));

  await emit("recording-progress", { seconds: 2, bytes: 192_044 });
  unlisten();

  expect(seen).toEqual([{ seconds: 2, bytes: 192_044 }]);
});
