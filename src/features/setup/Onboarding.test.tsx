import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ModelsView } from "../../ipc/models";
import type { NotesFolder, SetupStatus } from "../../ipc/setup";
import { Onboarding } from "./Onboarding";

afterEach(async () => {
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
});

const defaultFolder: NotesFolder = {
  path: "/Users/someone/Documents/Anchovy",
  display: "~/Documents/Anchovy",
  exists: false,
  obsidian_vault: false,
};

const vault: NotesFolder = {
  path: "/Users/someone/Vaults/Work",
  display: "~/Vaults/Work",
  exists: true,
  obsidian_vault: true,
};

const fresh: SetupStatus = {
  notes_folder: null,
  default_folder: defaultFolder,
  microphone: "not_asked",
  computer_audio: "not_asked",
  finished: false,
  can_record: false,
};

const models: ModelsView = {
  memory_bytes: 16 * 1024 ** 3,
  models: [
    {
      id: "qwen3-asr-1.7b",
      role: "transcribe",
      display_name: "Qwen3-ASR 1.7B",
      languages: [],
      size_bytes: 3.4e9,
      min_ram_gb: 8,
      license: "Apache-2.0",
      fit: "recommended",
      selected: true,
      state: { kind: "not_downloaded" },
    },
    {
      id: "qwen3-4b",
      role: "summarize",
      display_name: "Qwen3-4B-Instruct-2507",
      languages: [],
      size_bytes: 2.3e9,
      min_ram_gb: 8,
      license: "Apache-2.0",
      fit: "recommended",
      selected: true,
      state: { kind: "not_downloaded" },
    },
  ],
};

type Answers = Record<string, unknown>;

function fake(answers: Answers) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const answer = answers[cmd];
      if (answer instanceof Error) throw answer;
      return typeof answer === "function" ? answer(args) : answer;
    },
    { shouldMockEvents: true },
  );
  return calls;
}

const names = (calls: { cmd: string }[]) => calls.map(({ cmd }) => cmd);

test("step 1 offers ~/Documents/Anchovy and Continue creates it", async () => {
  const calls = fake({ use_default_notes_folder: { ...defaultFolder, exists: true } });
  render(<Onboarding initial={fresh} onDone={() => {}} />);

  expect(screen.getByText("Anchovy setup · Step 1 of 3")).toBeInTheDocument();
  expect(screen.getByText("~/Documents/Anchovy")).toBeInTheDocument();
  expect(screen.getByText("New folder. Anchovy creates it when you continue.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));

  expect(await screen.findByRole("heading", { name: "Allow audio" })).toBeInTheDocument();
  expect(names(calls)).toEqual(["use_default_notes_folder"]);
});

test("step 1 can use an existing Obsidian vault", async () => {
  const calls = fake({ choose_notes_folder: vault });
  render(<Onboarding initial={fresh} onDone={() => {}} />);

  fireEvent.click(screen.getByRole("button", { name: "Choose Folder…" }));

  expect(await screen.findByText("~/Vaults/Work")).toBeInTheDocument();
  expect(
    screen.getByText("Obsidian vault. Each recording gets its own folder inside it."),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByRole("heading", { name: "Allow audio" })).toBeInTheDocument();
  // The vault is already chosen; Continue does not ask again.
  expect(names(calls)).toEqual(["choose_notes_folder"]);
});

test("cancelling the folder panel stays on step 1", async () => {
  fake({ use_default_notes_folder: null });
  render(<Onboarding initial={fresh} onDone={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await act(() => Promise.resolve());
  expect(screen.getByRole("heading", { name: "Choose a notes folder" })).toBeInTheDocument();
});

test("step 2 asks for the microphone first, then computer audio", async () => {
  const calls = fake({ request_microphone: "allowed", check_computer_audio: "allowed" });
  render(<Onboarding initial={{ ...fresh, notes_folder: defaultFolder }} onDone={() => {}} />);

  expect(screen.getByText("Records your voice. Needed to record.")).toBeInTheDocument();
  expect(screen.getByText("Records the other side of calls.")).toBeInTheDocument();
  const [microphone, computerAudio] = screen.getAllByRole("button", { name: "Allow" });
  expect(computerAudio).toBeDisabled();
  expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();

  fireEvent.click(microphone);
  expect(await screen.findByText("Allowed")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();

  fireEvent.click(screen.getByRole("button", { name: "Allow" }));
  await vi.waitFor(() => expect(screen.getAllByText("Allowed")).toHaveLength(2));
  expect(calls).toEqual([
    { cmd: "request_microphone", args: {} },
    { cmd: "check_computer_audio", args: { ask: true } },
  ]);
});

test("a relaunch starts at the first step still to do", () => {
  fake({ list_models: models });
  render(
    <Onboarding
      initial={{ ...fresh, notes_folder: defaultFolder, microphone: "allowed" }}
      onDone={() => {}}
    />,
  );
  expect(screen.getByRole("heading", { name: "Download the models" })).toBeInTheDocument();
});

test("denied computer audio: Continue still works and the screen says what is lost", async () => {
  const calls = fake({
    request_microphone: "allowed",
    check_computer_audio: "denied",
    list_models: models,
  });
  render(<Onboarding initial={{ ...fresh, notes_folder: defaultFolder }} onDone={() => {}} />);

  fireEvent.click(screen.getAllByRole("button", { name: "Allow" })[0]);
  await screen.findByText("Allowed");
  fireEvent.click(screen.getByRole("button", { name: "Allow" }));

  expect(await screen.findByText("Not allowed")).toBeInTheDocument();
  expect(
    screen.getByText(
      "You can continue. Anchovy will record only your microphone, so the other side of online meetings will not be recorded.",
    ),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }));
  expect(calls).toContainEqual({ cmd: "open_privacy_settings", args: { pane: "computer_audio" } });

  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByRole("heading", { name: "Download the models" })).toBeInTheDocument();
});

test("a denied microphone says Record will not work", async () => {
  fake({ request_microphone: "denied" });
  render(<Onboarding initial={{ ...fresh, notes_folder: defaultFolder }} onDone={() => {}} />);
  fireEvent.click(screen.getAllByRole("button", { name: "Allow" })[0]);
  expect(
    await screen.findByText(
      "Anchovy can't record without the microphone. Allow Anchovy under Microphone in System Settings.",
    ),
  ).toBeInTheDocument();
});

test("coming back to the window checks computer audio again", async () => {
  const calls = fake({ check_computer_audio: "allowed" });
  render(
    <Onboarding
      initial={{
        ...fresh,
        notes_folder: defaultFolder,
        microphone: "not_asked",
        computer_audio: "denied",
      }}
      onDone={() => {}}
    />,
  );
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(await screen.findByText("Allowed")).toBeInTheDocument();
  expect(calls).toEqual([{ cmd: "check_computer_audio", args: { ask: false } }]);
});

const atModels: SetupStatus = {
  ...fresh,
  notes_folder: defaultFolder,
  microphone: "allowed",
  computer_audio: "allowed",
};

test("step 3 shows both default models and the total size", async () => {
  fake({ list_models: models });
  render(<Onboarding initial={atModels} onDone={() => {}} />);

  expect(await screen.findByText("Qwen3-ASR 1.7B")).toBeInTheDocument();
  expect(screen.getByText("Transcription · Apache-2.0")).toBeInTheDocument();
  expect(screen.getByText("Summary · Apache-2.0")).toBeInTheDocument();
  expect(
    screen.getByText("Total 5.7 GB. Anchovy checks each file after it downloads."),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Download 5.7 GB" })).toBeInTheDocument();
});

test("Later finishes setup without downloading", async () => {
  const onDone = vi.fn();
  const calls = fake({ list_models: models, finish_setup: null });
  render(<Onboarding initial={atModels} onDone={onDone} />);
  await screen.findByText("Qwen3-ASR 1.7B");

  fireEvent.click(screen.getByRole("button", { name: "Later" }));

  await vi.waitFor(() => expect(onDone).toHaveBeenCalledOnce());
  expect(names(calls)).toContain("finish_setup");
  expect(names(calls)).not.toContain("download_model");
});

test("Download fetches one model at a time and shows progress", async () => {
  const calls = fake({ list_models: models, download_model: null });
  render(<Onboarding initial={atModels} onDone={() => {}} />);
  await screen.findByText("Qwen3-ASR 1.7B");

  fireEvent.click(screen.getByRole("button", { name: "Download 5.7 GB" }));
  await vi.waitFor(() => expect(names(calls)).toContain("download_model"));
  expect(calls.filter(({ cmd }) => cmd === "download_model")).toEqual([
    { cmd: "download_model", args: { id: "qwen3-asr-1.7b" } },
  ]);
  expect(screen.getByText("Waiting")).toBeInTheDocument();

  await act(() => emit("model-progress", { id: "qwen3-asr-1.7b", downloaded: 2.1e9 }));
  expect(screen.getByText("2.1 of 3.4 GB")).toBeInTheDocument();
  expect(screen.getByText("Downloading 2.1 of 5.7 GB")).toBeInTheDocument();
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "37");
  // The download carries on after Continue.
  expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: "Later" })).not.toBeInTheDocument();

  await act(() => emit("models-changed", "qwen3-asr-1.7b"));
  await vi.waitFor(() =>
    expect(calls.filter(({ cmd }) => cmd === "download_model")).toHaveLength(2),
  );
  expect(calls.filter(({ cmd }) => cmd === "download_model")[1]).toEqual({
    cmd: "download_model",
    args: { id: "qwen3-4b" },
  });
});
