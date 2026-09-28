import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { ModelsView, ModelView } from "../../ipc/models";
import { ModelsScreen } from "./ModelsScreen";

const GIB = 1024 ** 3;

function model(overrides: Partial<ModelView> & Pick<ModelView, "id">): ModelView {
  return {
    role: "transcribe",
    display_name: overrides.id,
    languages: ["Chinese", "English"],
    size_bytes: 2_520_744_288,
    min_ram_gb: 8,
    license: "Apache-2.0",
    fit: "fits",
    selected: false,
    state: { kind: "not_downloaded" },
    ...overrides,
  };
}

function view(models: ModelView[], memoryGb = 8): ModelsView {
  return { memory_bytes: memoryGb * GIB, models };
}

type Call = { cmd: string; id?: string };

// Answers the model commands from `current`, which tests can replace.
function fakeBackend(initial: ModelsView) {
  const backend = { current: initial, calls: [] as Call[] };
  mockIPC(
    (cmd, args) => {
      const id = (args as { id?: string } | undefined)?.id;
      backend.calls.push(id ? { cmd, id } : { cmd });
      if (cmd === "list_models") return backend.current;
      return null;
    },
    { shouldMockEvents: true },
  );
  return backend;
}

const commandsOnly = (calls: Call[]) => calls.filter((call) => call.cmd !== "list_models");

// Clicks, then lets the resulting IPC calls and state updates settle.
async function click(element: HTMLElement) {
  await act(async () => {
    fireEvent.click(element);
  });
}

function row(name: string) {
  return screen.getByRole("group", { name });
}

afterEach(async () => {
  // Unmount and let the screen stop listening before the mocks go away.
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
});

test("shows this Mac's memory and each model's label, grouped by job", async () => {
  fakeBackend(
    view([
      model({ id: "Qwen3-ASR 1.7B", fit: "fits", selected: true }),
      model({ id: "Qwen3-ASR 0.6B", fit: "recommended", min_ram_gb: 4 }),
      model({ id: "Qwen3-4B Instruct 2507", role: "summarize", selected: true }),
      model({ id: "Qwen3-4B 8-bit", role: "summarize", fit: "too_large", min_ram_gb: 16 }),
    ]),
  );

  render(<ModelsScreen />);

  expect(await screen.findByText("This Mac has 8 GB of memory.")).toBeInTheDocument();
  const transcription = screen.getByRole("region", { name: "Transcription" });
  const summary = screen.getByRole("region", { name: "Summary" });
  expect(within(transcription).getByRole("group", { name: "Qwen3-ASR 1.7B" })).toHaveTextContent(
    "Fits",
  );
  expect(within(transcription).getByRole("group", { name: "Qwen3-ASR 0.6B" })).toHaveTextContent(
    "Recommended",
  );
  expect(within(summary).getByRole("group", { name: "Qwen3-4B 8-bit" })).toHaveTextContent(
    "Too large",
  );
  expect(row("Qwen3-ASR 1.7B")).toHaveTextContent("2.5 GB");
  expect(row("Qwen3-ASR 1.7B")).toHaveTextContent("Chinese, English");
  expect(row("Qwen3-ASR 1.7B")).toHaveTextContent("Apache-2.0");
});

test("models can only be picked from the list, never typed in", async () => {
  fakeBackend(view([model({ id: "A", selected: true }), model({ id: "B" })]));

  render(<ModelsScreen />);

  await screen.findByRole("group", { name: "A" });
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(screen.getByRole("radio", { name: "Use A" })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Use B" })).not.toBeChecked();
});

test("picking another model selects it and refreshes the list", async () => {
  const backend = fakeBackend(view([model({ id: "A", selected: true }), model({ id: "B" })]));
  render(<ModelsScreen />);
  const b = await screen.findByRole("radio", { name: "Use B" });

  backend.current = view([model({ id: "A" }), model({ id: "B", selected: true })]);
  await click(b);

  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "select_model", id: "B" }]);
  expect(await screen.findByRole("radio", { name: "Use B" })).toBeChecked();
});

test("picking a model that is too large asks first", async () => {
  const backend = fakeBackend(
    view([
      model({ id: "A", selected: true }),
      model({ id: "Big", fit: "too_large", min_ram_gb: 16 }),
    ]),
  );
  render(<ModelsScreen />);
  await click(await screen.findByRole("radio", { name: "Use Big" }));

  expect(row("Big")).toHaveTextContent(
    "Big needs 16 GB of memory. This Mac has 8 GB, so notes may fail or take much longer.",
  );
  expect(commandsOnly(backend.calls)).toEqual([]);

  await click(within(row("Big")).getByRole("button", { name: "Cancel" }));
  expect(commandsOnly(backend.calls)).toEqual([]);
  expect(screen.getByRole("radio", { name: "Use A" })).toBeChecked();

  await click(screen.getByRole("radio", { name: "Use Big" }));
  await click(within(row("Big")).getByRole("button", { name: "Use anyway" }));
  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "select_model", id: "Big" }]);
});

test("downloading shows progress from Rust and becomes ready when it finishes", async () => {
  const backend = fakeBackend(view([model({ id: "A", selected: true })]));
  render(<ModelsScreen />);

  backend.current = view([
    model({ id: "A", selected: true, state: { kind: "downloading", downloaded: 0 } }),
  ]);
  await click(await screen.findByRole("button", { name: "Download" }));
  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "download_model", id: "A" }]);
  expect(await within(row("A")).findByRole("button", { name: "Cancel" })).toBeInTheDocument();

  await act(() => emit("model-progress", { id: "A", downloaded: 1_260_000_000 }));
  expect(row("A")).toHaveTextContent("1.3 GB of 2.5 GB");
  expect(within(row("A")).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "50");

  backend.current = view([model({ id: "A", selected: true, state: { kind: "ready" } })]);
  await act(() => emit("models-changed", "A"));
  expect(await within(row("A")).findByText("Downloaded")).toBeInTheDocument();
  expect(within(row("A")).getByRole("button", { name: "Delete" })).toBeInTheDocument();
});

test("cancelling a download keeps what was downloaded", async () => {
  const backend = fakeBackend(
    view([model({ id: "A", state: { kind: "downloading", downloaded: 100 } })]),
  );
  render(<ModelsScreen />);

  backend.current = view([
    model({ id: "A", state: { kind: "paused", downloaded: 1_000_000_000 } }),
  ]);
  await click(
    await within(await screen.findByRole("group", { name: "A" })).findByRole("button", {
      name: "Cancel",
    }),
  );

  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "cancel_model_download", id: "A" }]);
  await act(() => emit("models-changed", "A"));
  expect(await within(row("A")).findByText("1.0 GB of 2.5 GB downloaded")).toBeInTheDocument();
  expect(within(row("A")).getByRole("button", { name: "Resume" })).toBeInTheDocument();
});

test("a failed download says why and can be retried", async () => {
  const backend = fakeBackend(
    view([
      model({
        id: "A",
        state: {
          kind: "failed",
          downloaded: 0,
          error: "A.gguf did not match its checksum and was removed. Download it again.",
        },
      }),
    ]),
  );
  render(<ModelsScreen />);

  const a = await screen.findByRole("group", { name: "A" });
  expect(a).toHaveTextContent("A.gguf did not match its checksum and was removed.");
  expect(a).not.toHaveTextContent("Downloaded");
  await click(within(a).getByRole("button", { name: "Retry" }));
  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "download_model", id: "A" }]);
});

test("downloading a model that is too large asks first", async () => {
  const backend = fakeBackend(view([model({ id: "Big", fit: "too_large", min_ram_gb: 16 })]));
  render(<ModelsScreen />);

  await click(await screen.findByRole("button", { name: "Download" }));
  expect(commandsOnly(backend.calls)).toEqual([]);
  await click(within(row("Big")).getByRole("button", { name: "Download anyway" }));
  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "download_model", id: "Big" }]);
});

test("deleting a downloaded model asks first", async () => {
  const backend = fakeBackend(view([model({ id: "A", state: { kind: "ready" } })]));
  render(<ModelsScreen />);

  await click(await screen.findByRole("button", { name: "Delete" }));
  expect(row("A")).toHaveTextContent("Delete A from this Mac? You can download it again later.");
  expect(commandsOnly(backend.calls)).toEqual([]);

  backend.current = view([model({ id: "A" })]);
  await click(within(row("A")).getByRole("button", { name: "Delete" }));
  expect(commandsOnly(backend.calls)).toEqual([{ cmd: "delete_model", id: "A" }]);
  expect(await within(row("A")).findByRole("button", { name: "Download" })).toBeInTheDocument();
});

test("an error from Rust is shown instead of failing silently", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "list_models") return view([model({ id: "A", state: { kind: "ready" } })]);
      if (cmd === "delete_model") throw "That model is downloading.";
    },
    { shouldMockEvents: true },
  );
  render(<ModelsScreen />);

  await click(await screen.findByRole("button", { name: "Delete" }));
  await click(within(row("A")).getByRole("button", { name: "Delete" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("That model is downloading.");
});
