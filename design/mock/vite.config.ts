import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const here = import.meta.dirname;

// Static design mock: the app's React, Tailwind, and Geist, with fake data.
// It never loads Tauri or calls Rust.
export default defineConfig({
  root: here,
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1440,
    strictPort: true,
    // Serve fonts from the repository's node_modules.
    fs: { allow: [path.resolve(here, "../..")] },
  },
});
