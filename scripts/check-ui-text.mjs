// Interface text is English only. Fails when user-facing source under src/ or
// index.html contains Chinese, Japanese, or Korean characters. Tests and
// fixtures may contain them.
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cjkScripts = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
// CJK punctuation and full-width forms.
const cjkPunctuation = [
  [0x3001, 0x303f],
  [0xff01, 0xff60],
];
const hasCjk = (line) =>
  cjkScripts.test(line) ||
  [...line].some((char) => {
    const code = char.codePointAt(0);
    return cjkPunctuation.some(([low, high]) => code >= low && code <= high);
  });
const isTestFile = (file) => /\.test\.[jt]sx?$/.test(file) || /(^|\/)(test|fixtures)\//.test(file);

const files = ["index.html"];
for (const entry of readdirSync(path.join(root, "src"), { recursive: true })) {
  const file = path.join("src", entry);
  if (/\.(tsx?|jsx?|html|css|json)$/.test(file) && !isTestFile(file)) files.push(file);
}

const problems = [];
for (const file of files) {
  readFileSync(path.join(root, file), "utf8")
    .split("\n")
    .forEach((line, index) => {
      if (hasCjk(line)) problems.push(`${file}:${index + 1}: ${line.trim()}`);
    });
}

if (problems.length) {
  console.error("Interface text must be English only:");
  for (const problem of problems) console.error(`  ${problem}`);
  process.exit(1);
}
console.log(`Interface text check passed: ${files.length} files.`);
