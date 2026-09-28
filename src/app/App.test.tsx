import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";

afterEach(async () => {
  // Unmount and let the Models screen stop listening before the mocks go away.
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
});

test("shows the name and version returned by the Rust command", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "app_info") return { name: "Anchovy", version: "9.9.9" };
  });

  render(<App />);

  expect(await screen.findByText("Anchovy 9.9.9")).toBeInTheDocument();
  expect(calls).toEqual(["app_info"]);
});

test("the Models button opens the Models screen and closes it again", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "app_info") return { name: "Anchovy", version: "9.9.9" };
      if (cmd === "list_models") return { memory_bytes: 16 * 1024 ** 3, models: [] };
    },
    { shouldMockEvents: true },
  );
  render(<App />);
  const button = await screen.findByRole("button", { name: "Models" });
  expect(button).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(button);

  expect(await screen.findByRole("heading", { name: "Models" })).toBeInTheDocument();
  expect(await screen.findByText("This Mac has 16 GB of memory.")).toBeInTheDocument();
  expect(button).toHaveAttribute("aria-pressed", "true");

  fireEvent.click(button);
  expect(screen.queryByRole("heading", { name: "Models" })).not.toBeInTheDocument();
});
