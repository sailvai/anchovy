// Privacy check, part of `npm run verify`. Fails when:
// - code or configuration contains an external address not in privacy/allowed-urls.txt
// - a dependency is an analytics, crash-reporting, or advertising SDK
// - the Tauri CSP allows an external address, or the private macOS API is on
// - an npm dependency or a models.json entry has a license outside privacy/deny.toml
// Rust dependency licenses are checked by `cargo deny` in verify.
import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const problems = [];

// Files a contributor could add an address to: tracked plus new, not ignored.
const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard"], {
  cwd: root,
  encoding: "utf8",
})
  .split("\n")
  .filter(
    (file) =>
      file && existsSync(path.join(root, file)) && lstatSync(path.join(root, file)).isFile(),
  );

// 1. External addresses.
const allowedUrls = read("privacy/allowed-urls.txt")
  .split("\n")
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith("#"));
const skipUrlScan = [
  /\.md$/, // prose, not code
  /(^|\/)package-lock\.json$/,
  /(^|\/)Cargo\.lock$/,
  /\.(png|icns|ico|jpg|jpeg|gif|webp|woff2?|ttf|wav|m4a)$/,
  /^privacy\/allowed-urls\.txt$/,
];
const urlPattern = /\b(?:https?|wss?):\/\/[^\s"'`<>()\\]+/g;
for (const file of files) {
  if (skipUrlScan.some((pattern) => pattern.test(file))) continue;
  const lines = read(file).split("\n");
  lines.forEach((line, index) => {
    for (const [url] of line.matchAll(urlPattern)) {
      if (!allowedUrls.some((prefix) => url.startsWith(prefix))) {
        problems.push(`${file}:${index + 1}: external address not in the allowlist: ${url}`);
      }
    }
  });
}

// 2. Telemetry and advertising SDKs.
const trackerPattern =
  /(^|[/@_-])(sentry|posthog|mixpanel|amplitude|segment|analytics|datadog|bugsnag|rollbar|logrocket|newrelic|honeybadger|appcenter|applicationinsights|firebase|crashlytics|fullstory|hotjar|heap|telemetry|admob|adsense|google-ads)([/_-]|$)/i;
const lock = JSON.parse(read("package-lock.json"));
const npmPackages = Object.entries(lock.packages ?? {}).filter(([key]) => key !== "");
for (const [key] of npmPackages) {
  const name = key.replace(/^.*node_modules\//, "");
  if (trackerPattern.test(name)) problems.push(`npm dependency looks like a tracker: ${name}`);
}
if (existsSync(path.join(root, "src-tauri/Cargo.lock"))) {
  for (const [, name] of read("src-tauri/Cargo.lock").matchAll(/^name = "([^"]+)"/gm)) {
    if (trackerPattern.test(name)) problems.push(`Rust dependency looks like a tracker: ${name}`);
  }
}

// 3. CSP and private API in every Tauri config.
const allowedCspSources = new Set([
  "'self'",
  "'none'",
  "'unsafe-inline'",
  "ipc:",
  "http://ipc.localhost",
  "asset:",
  "http://asset.localhost",
  "data:",
  "blob:",
]);
const tauriConfigs = readdirSync(path.join(root, "src-tauri")).filter((file) =>
  /^tauri(\..+)?\.conf\.json$/.test(file),
);
for (const file of tauriConfigs) {
  const config = JSON.parse(read(`src-tauri/${file}`));
  const security = config.app?.security;
  if (file === "tauri.conf.json" && typeof security?.csp !== "string") {
    problems.push(`src-tauri/${file}: app.security.csp must be set`);
  }
  for (const key of ["csp", "devCsp"]) {
    const csp = security?.[key];
    if (csp == null) continue;
    const text =
      typeof csp === "string"
        ? csp
        : Object.entries(csp)
            .map(([d, v]) => `${d} ${v}`)
            .join(";");
    for (const directive of text.split(";")) {
      const [, ...sources] = directive.trim().split(/\s+/);
      for (const source of sources) {
        if (!allowedCspSources.has(source)) {
          problems.push(`src-tauri/${file}: ${key} allows ${source}`);
        }
      }
    }
  }
  if (config.app?.macOSPrivateApi === true) {
    problems.push(`src-tauri/${file}: macOSPrivateApi must stay off`);
  }
}
if (/macos-private-api/.test(read("src-tauri/Cargo.toml"))) {
  problems.push("src-tauri/Cargo.toml: the macos-private-api feature must stay off");
}

// 4. Licenses: npm dependencies and models.json, against the cargo-deny list.
const allowedLicenses = new Set(
  [
    ...read("privacy/deny.toml")
      .match(/allow = \[([\s\S]*?)\]/)[1]
      .matchAll(/"([^"]+)"/g),
  ].map((match) => match[1]),
);
// Minimal SPDX expression check: OR needs one side, AND needs both.
function licenseAllowed(expression) {
  const text = expression
    .trim()
    .replace(/^\((.*)\)$/, "$1")
    .trim();
  if (allowedLicenses.has(text)) return true;
  let depth = 0;
  for (const operator of [" OR ", " AND "]) {
    const parts = [];
    let start = 0;
    for (let i = 0; i < text.length; i++) {
      if (text[i] === "(") depth++;
      if (text[i] === ")") depth--;
      if (depth === 0 && text.startsWith(operator, i)) {
        parts.push(text.slice(start, i));
        start = i + operator.length;
      }
    }
    if (parts.length) {
      parts.push(text.slice(start));
      return operator === " OR " ? parts.some(licenseAllowed) : parts.every(licenseAllowed);
    }
  }
  const [base] = text.split(" WITH ");
  return base !== text && allowedLicenses.has(base.trim());
}
for (const [key, entry] of npmPackages) {
  const name = key.replace(/^.*node_modules\//, "");
  if (!entry.license) problems.push(`npm dependency has no license: ${name}`);
  else if (!licenseAllowed(entry.license)) {
    problems.push(`npm dependency ${name} has license ${entry.license}, not in privacy/deny.toml`);
  }
}
const modelsFile = "src-tauri/resources/models.json";
if (existsSync(path.join(root, modelsFile))) {
  const models = JSON.parse(read(modelsFile));
  for (const model of Array.isArray(models) ? models : (models.models ?? [])) {
    if (!model.license || !licenseAllowed(model.license)) {
      problems.push(`${modelsFile}: ${model.id} has license ${model.license ?? "(none)"}`);
    }
  }
}

if (problems.length) {
  problems.splice(0, problems.length, ...new Set(problems));
  console.error(`Privacy check failed (${problems.length}):`);
  for (const problem of problems) console.error(`  ${problem}`);
  process.exit(1);
}
console.log(
  `Privacy check passed: ${files.length} files scanned, ${npmPackages.length} npm packages, ${tauriConfigs.length} Tauri config.`,
);
