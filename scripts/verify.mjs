// `npm run verify`: every check an agent or CI runs before saying "done".
// Runs each step in order, keeps going after a failure so one run shows
// everything, and exits non-zero if any step failed.
//
//   npm run verify                  all steps
//   npm run verify -- --skip-build  everything except the final tauri build
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(root, "src-tauri");
const skipBuild = process.argv.includes("--skip-build");
// Outside test-results/, which Playwright clears on every run.
const vitestReport = path.join(root, "node_modules", ".tmp", "vitest-report.json");

const steps = [
  ["Format (Prettier)", "npx", ["prettier", "--check", "."]],
  ["Lint (ESLint)", "npx", ["eslint", ".", "--max-warnings", "0"]],
  ["Types and interface build", "npm", ["run", "build"]],
  [
    "Interface tests (Vitest)",
    "npx",
    ["vitest", "run", "--reporter=default", "--reporter=json", `--outputFile=${vitestReport}`],
  ],
  ["Rust format", "cargo", ["fmt", "--check"], tauriDir],
  ["Rust lint (Clippy)", "cargo", ["clippy", "--all-targets", "--", "-D", "warnings"], tauriDir],
  ["Rust tests", "cargo", ["test"], tauriDir, { capture: true }],
  [
    "Rust licenses (cargo deny)",
    "cargo",
    ["deny", "--config", "../privacy/deny.toml", "check", "licenses", "sources"],
    tauriDir,
  ],
  ["Privacy check", "node", ["scripts/check-privacy.mjs"]],
  ["Interface text check", "node", ["scripts/check-ui-text.mjs"]],
  ["Screenshots (Playwright)", "npx", ["playwright", "test"]],
];
if (!skipBuild) {
  steps.push([
    "Unsigned app build (tauri build)",
    "npx",
    ["tauri", "build", "--bundles", "app", "--target", "aarch64-apple-darwin", "--no-sign"],
  ]);
}

const results = [];
let rustIgnored = 0;
for (const [name, command, args, cwd = root, options = {}] of steps) {
  console.log(`\n=== ${name}: ${command} ${args.join(" ")}`);
  const started = Date.now();
  const run = spawnSync(command, args, {
    cwd,
    stdio: options.capture ? ["inherit", "pipe", "inherit"] : "inherit",
    encoding: "utf8",
  });
  if (options.capture) {
    process.stdout.write(run.stdout ?? "");
    for (const [, count] of (run.stdout ?? "").matchAll(/(\d+) ignored/g)) {
      rustIgnored += Number(count);
    }
  }
  const ok = run.status === 0;
  results.push({ name, ok, seconds: ((Date.now() - started) / 1000).toFixed(1) });
}

// Skipped tests must be explained in the pull request, so always report them.
let vitestSkipped = "unknown";
if (existsSync(vitestReport)) {
  const report = JSON.parse(readFileSync(vitestReport, "utf8"));
  vitestSkipped = report.numPendingTests + report.numTodoTests;
}

console.log("\n=== verify summary");
for (const { name, ok, seconds } of results) {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name} (${seconds}s)`);
}
if (skipBuild) console.log("SKIP  Unsigned app build (--skip-build)");
console.log(`Skipped tests: Vitest ${vitestSkipped}, Rust ${rustIgnored}`);

const failed = results.filter((result) => !result.ok);
if (failed.length) {
  console.log(`verify FAILED: ${failed.map((result) => result.name).join(", ")}`);
  process.exit(1);
}
console.log("verify passed");
