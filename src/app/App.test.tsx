import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";

afterEach(() => {
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
