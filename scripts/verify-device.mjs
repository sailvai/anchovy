// `npm run verify:device`: the recording loopback test (plan 8.4). Needs a
// real Mac with audio hardware and permissions, so it never runs in CI.
//
// Plays a known test tone with afplay (another process, as a meeting app
// would be), starting 1.5 s into a 5-second recording by the app's own
// recording module, then
// checks automatically that the computer-audio stream was captured and the
// tone is in the saved file. It also runs the first-launch computer audio
// check, which must report the permission as given. The microphone needs a person speaking; that
// check is manual (plan 8.9) and is only reported here.
//
//   npm run verify:device                  run from this terminal, unsandboxed
//   npm run verify:device -- --sandboxed   also run inside an ad-hoc signed,
//                                          sandboxed .app with the app's
//                                          Entitlements.plist and Info.plist
//
// macOS asks for Microphone and System Audio Recording permission the first
// time the sandboxed .app runs. Someone must click Allow; until then the tap
// delivers silence and the check fails.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(root, "src-tauri");
const sandboxed = process.argv.includes("--sandboxed");
const seconds = 5;
const toneHz = 997;
const work = path.join(os.tmpdir(), "anchovy-verify-device");
const bundleId = "com.sailvai.anchovy.loopback";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", ...options });
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (${result.status}):\n${result.stderr ?? ""}`,
    );
  }
  return result.stdout;
}

function build() {
  console.log("=== Build the loopback recorder (cargo build --example loopback --release)");
  run("cargo", ["build", "--release", "--example", "loopback"], {
    cwd: tauriDir,
    stdio: "inherit",
  });
  return path.join(tauriDir, "target", "release", "examples", "loopback");
}

// Starts `record`, then plays the tone 1.5 s in: the recording must run
// through the silent start (the Mac is often quiet when Record is pressed)
// and pick up computer audio once it begins.
async function withTone(binary, record) {
  const tone = path.join(work, "tone.wav");
  run(binary, ["tone", tone, "--seconds", String(seconds), "--tone-hz", String(toneHz)]);
  let player = null;
  const timer = setTimeout(() => {
    player = spawn("afplay", [tone], { stdio: "ignore" });
  }, 1500);
  try {
    return await record();
  } finally {
    clearTimeout(timer);
    player?.kill();
  }
}

function collect(child) {
  return new Promise((resolve) => {
    let stdout = "";
    child.stdout?.on("data", (chunk) => (stdout += chunk));
    child.on("close", (status) => resolve({ status, stdout }));
  });
}

function report(name, output, status) {
  console.log(output.trim());
  let parsed = null;
  try {
    parsed = JSON.parse(output);
  } catch {
    // Reported as a failure below.
  }
  const ok = status === 0 && parsed?.passed === true;
  return { name, ok };
}

async function unsandboxed(binary) {
  console.log(`\n=== Loopback, unsandboxed: record ${seconds} s while afplay plays ${toneHz} Hz`);
  const out = path.join(work, "unsandboxed");
  rmSync(out, { recursive: true, force: true });
  const result = await withTone(binary, () =>
    collect(
      spawn(
        binary,
        ["record", "--seconds", String(seconds), "--tone-hz", String(toneHz), "--out", out],
        { stdio: ["ignore", "pipe", "inherit"] },
      ),
    ),
  );
  return report("Loopback, unsandboxed", result.stdout ?? "", result.status);
}

// The same binary wrapped in a .app with the app's entitlements and usage
// strings, so it runs in the App Sandbox and gets its own permission prompts.
function bundle(binary) {
  const app = path.join(work, "Anchovy Loopback.app");
  rmSync(app, { recursive: true, force: true });
  mkdirSync(path.join(app, "Contents", "MacOS"), { recursive: true });
  writeFileSync(path.join(app, "Contents", "MacOS", "loopback"), readFileSync(binary), {
    mode: 0o755,
  });
  const usage = readFileSync(path.join(tauriDir, "Info.plist"), "utf8").match(
    /<dict>([\s\S]*)<\/dict>/,
  )[1];
  writeFileSync(
    path.join(app, "Contents", "Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>loopback</string>
	<key>CFBundleIdentifier</key>
	<string>${bundleId}</string>
	<key>CFBundleName</key>
	<string>Anchovy Loopback</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>LSMinimumSystemVersion</key>
	<string>14.4</string>
	<key>LSUIElement</key>
	<true/>${usage}</dict>
</plist>
`,
  );
  run("codesign", [
    "--force",
    "--sign",
    "-",
    "--options",
    "runtime",
    "--entitlements",
    path.join(tauriDir, "Entitlements.plist"),
    app,
  ]);
  return app;
}

async function sandboxedRun(binary) {
  console.log(
    `\n=== Loopback, sandboxed .app: record ${seconds} s while afplay plays ${toneHz} Hz`,
  );
  const app = bundle(binary);
  const stdout = path.join(work, "sandboxed-out.json");
  const stderr = path.join(work, "sandboxed-err.txt");
  rmSync(stdout, { force: true });
  rmSync(stderr, { force: true });
  // Without --out the recorder writes to the temporary folder, which in the
  // sandbox is inside the app's container.
  const result = await withTone(binary, () =>
    collect(
      spawn(
        "open",
        [
          "-W",
          "-n",
          "--stdout",
          stdout,
          "--stderr",
          stderr,
          app,
          "--args",
          "record",
          "--seconds",
          String(seconds),
          "--tone-hz",
          String(toneHz),
        ],
        { stdio: "ignore", timeout: 120_000 },
      ),
    ),
  );
  if (existsSync(stderr)) process.stderr.write(readFileSync(stderr, "utf8"));
  const output = existsSync(stdout) ? readFileSync(stdout, "utf8") : "";
  const parsed = report("Loopback, sandboxed .app", output, result.status);
  const sandboxOk = output.includes('"sandboxed": true');
  if (!sandboxOk) console.log("The sandboxed run did not report that it ran in the App Sandbox.");
  return { ...parsed, ok: parsed.ok && sandboxOk };
}

// The first-launch computer audio check (plan step 3): a one-second muted
// tone from this process must reach a tap of it when the permission is given.
function probe(binary) {
  console.log("\n=== Computer audio check (first launch), unsandboxed");
  const result = spawnSync(binary, ["probe"], { cwd: root, encoding: "utf8" });
  process.stdout.write(result.stdout ?? "");
  process.stderr.write(result.stderr ?? "");
  return { name: "Computer audio check", ok: result.status === 0 };
}

mkdirSync(work, { recursive: true });
const binary = build();
const results = [probe(binary), await unsandboxed(binary)];
if (sandboxed) results.push(await sandboxedRun(binary));

console.log("\n=== verify:device summary");
for (const { name, ok } of results) console.log(`${ok ? "PASS" : "FAIL"}  ${name}`);
if (!sandboxed) console.log("SKIP  Loopback, sandboxed .app (run with -- --sandboxed)");
console.log("MANUAL  Microphone with a person speaking (plan 8.9, item 2)");
if (results.some((result) => !result.ok)) {
  console.log("verify:device FAILED");
  process.exit(1);
}
console.log("verify:device passed");
