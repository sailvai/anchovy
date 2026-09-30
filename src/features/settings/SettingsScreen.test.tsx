import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { InputDevice } from "../../ipc/recording";
import type { Settings } from "../../ipc/settings";
import type { NotesFolder } from "../../ipc/setup";
import { SettingsScreen } from "./SettingsScreen";

afterEach(() => {
  cleanup();
  clearMocks();
});

const folder: NotesFolder = {
  path: "/Users/someone/Documents/Anchovy",
  display: "~/Documents/Anchovy",
  exists: true,
  obsidian_vault: false,
};

const vault: NotesFolder = {
  path: "/Users/someone/Notes/Vault",
  display: "~/Notes/Vault",
  exists: true,
  obsidian_vault: true,
};

// In system order; the default is not first.
const devices: InputDevice[] = [
  { uid: "AppleUSBAudioEngine:DJI", name: "Wireless Mic Rx", is_default: false },
  { uid: "BuiltInMicrophoneDevice", name: "MacBook Pro Microphone", is_default: true },
  { uid: "ZoomAudioDevice", name: "Zoom Audio", is_default: false },
];

const defaults: Settings = {
  input_device: null,
  recording_quality: "high",
  generate_notes_automatically: true,
};

type Call = { cmd: string; args: unknown };

function fakeSettings({
  saved = defaults,
  chosen = vault,
  saveError,
}: { saved?: Settings; chosen?: NotesFolder | null; saveError?: string } = {}) {
  const calls: Call[] = [];
  let current = saved;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "get_settings":
        return current;
      case "update_settings":
        if (saveError) throw saveError;
        current = (args as { settings: Settings }).settings;
        return current;
      case "list_input_devices":
        return devices;
      case "choose_notes_folder":
        return chosen;
    }
  });
  return calls;
}

function renderScreen(recording = false, onNotesFolderChanged = vi.fn()) {
  render(
    <SettingsScreen
      notesFolder={folder}
      recording={recording}
      onNotesFolderChanged={onNotesFolderChanged}
    />,
  );
  return onNotesFolderChanged;
}

const updates = (calls: Call[]) =>
  calls
    .filter(({ cmd }) => cmd === "update_settings")
    .map(({ args }) => (args as { settings: Settings }).settings);

test("exactly four rows, with the mock's titles and descriptions", async () => {
  fakeSettings();
  renderScreen();

  expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
  const rows = await screen.findAllByRole("group");
  expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual([
    "Notes folder",
    "Input device",
    "Recording quality",
    "Generate notes automatically",
  ]);
  for (const text of [
    "Each recording gets its own folder here.",
    "Mixed with computer audio when allowed.",
    "Applies to the next recording.",
    "When off, use Generate note.",
  ]) {
    expect(screen.getByText(text)).toBeInTheDocument();
  }
});

test("the rows show the saved values", async () => {
  fakeSettings();
  renderScreen();

  expect(await screen.findByText("~/Documents/Anchovy")).toBeInTheDocument();
  const device = await screen.findByRole("combobox", { name: "Input device" });
  expect(device).toHaveDisplayValue("MacBook Pro Microphone");
  expect(screen.getByRole("radio", { name: /^High/ })).toBeChecked();
  expect(screen.getByRole("radio", { name: /^Small/ })).not.toBeChecked();
  expect(screen.getByText("WAV, 48 kHz, 16-bit, mono")).toBeInTheDocument();
  expect(screen.getByText("M4A")).toBeInTheDocument();
  expect(screen.getByRole("switch", { name: "Generate notes automatically" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
});

test("the device menu lists inputs by their system names, the system default first", async () => {
  const calls = fakeSettings();
  renderScreen();
  const device = await screen.findByRole("combobox", { name: "Input device" });

  expect(
    within(device)
      .getAllByRole("option")
      .map((option) => option.textContent),
  ).toEqual(["MacBook Pro Microphone", "Wireless Mic Rx", "Zoom Audio"]);

  fireEvent.change(device, { target: { value: "AppleUSBAudioEngine:DJI" } });
  await waitFor(() => expect(device).toHaveDisplayValue("Wireless Mic Rx"));
  // The first entry is the system default: no device is saved.
  fireEvent.change(device, { target: { value: "" } });
  await waitFor(() => expect(device).toHaveDisplayValue("MacBook Pro Microphone"));

  expect(updates(calls).map((settings) => settings.input_device)).toEqual([
    "AppleUSBAudioEngine:DJI",
    null,
  ]);
});

test("a saved device that is not connected shows the system default and stays saved", async () => {
  const calls = fakeSettings({ saved: { ...defaults, input_device: "UnpluggedMic" } });
  renderScreen();

  const device = await screen.findByRole("combobox", { name: "Input device" });
  await waitFor(() => expect(device).toHaveDisplayValue("MacBook Pro Microphone"));
  expect(updates(calls)).toEqual([]);
});

test("choosing Small saves it", async () => {
  const calls = fakeSettings();
  renderScreen();

  fireEvent.click(await screen.findByRole("radio", { name: /^Small/ }));

  await waitFor(() => expect(screen.getByRole("radio", { name: /^Small/ })).toBeChecked());
  expect(updates(calls)).toEqual([{ ...defaults, recording_quality: "small" }]);
});

test("the switch saves", async () => {
  const calls = fakeSettings();
  renderScreen();
  const toggle = await screen.findByRole("switch", { name: "Generate notes automatically" });

  fireEvent.click(toggle);

  await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  expect(updates(calls)).toEqual([{ ...defaults, generate_notes_automatically: false }]);
});

test("a setting that cannot be saved says why and keeps the old value", async () => {
  fakeSettings({ saveError: "Anchovy couldn't save the settings. Disk full." });
  renderScreen();
  const toggle = await screen.findByRole("switch", { name: "Generate notes automatically" });

  fireEvent.click(toggle);

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Anchovy couldn't save the settings. Disk full.",
  );
  expect(toggle).toHaveAttribute("aria-checked", "true");
});

test("Change… is disabled while recording; the other settings apply to the next one", async () => {
  fakeSettings();
  renderScreen(true);

  expect(await screen.findByRole("button", { name: "Change…" })).toBeDisabled();
  expect(await screen.findByRole("combobox", { name: "Input device" })).toBeEnabled();
  expect(screen.getByRole("radio", { name: /^Small/ })).toBeEnabled();
  expect(screen.getByRole("switch", { name: "Generate notes automatically" })).toBeEnabled();
});

test("Change… opens the folder panel and reports the new folder", async () => {
  const calls = fakeSettings();
  const changed = renderScreen(false);

  fireEvent.click(await screen.findByRole("button", { name: "Change…" }));

  await waitFor(() => expect(changed).toHaveBeenCalledWith(vault));
  expect(calls.map(({ cmd }) => cmd)).toContain("choose_notes_folder");
});

test("cancelling the folder panel changes nothing", async () => {
  fakeSettings({ chosen: null });
  const changed = renderScreen(false);

  fireEvent.click(await screen.findByRole("button", { name: "Change…" }));

  await waitFor(() => expect(screen.getByRole("button", { name: "Change…" })).toBeEnabled());
  expect(changed).not.toHaveBeenCalled();
});

test("two changes made before the first is saved both stay saved", async () => {
  const calls = fakeSettings();
  renderScreen();
  const toggle = await screen.findByRole("switch", { name: "Generate notes automatically" });

  fireEvent.click(toggle);
  fireEvent.click(screen.getByRole("radio", { name: /^Small/ }));

  await waitFor(() => expect(updates(calls)).toHaveLength(2));
  expect(updates(calls)[1]).toEqual({
    ...defaults,
    generate_notes_automatically: false,
    recording_quality: "small",
  });
});
