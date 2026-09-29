// Scoring for `npm run eval`: transcription error rates, whether each
// decision and action item is supported by what was said, the language of a
// summary, and the comparison with evals/baseline.json.

// Chinese is scored by characters, English by words. Punctuation and case
// are ignored.
export function units(text, language) {
  const clean = text.toLowerCase().replace(/[^\p{L}\p{N}']+/gu, " ");
  if (language === "zh") return [...clean.replace(/\s+/g, "")];
  return clean.split(/\s+/).filter(Boolean);
}

// Edit distance between two unit lists.
export function edits(reference, hypothesis) {
  let previous = Array.from({ length: hypothesis.length + 1 }, (_, j) => j);
  for (let i = 0; i < reference.length; i++) {
    const row = [i + 1];
    for (let j = 0; j < hypothesis.length; j++) {
      row.push(
        Math.min(
          previous[j] + (reference[i] === hypothesis[j] ? 0 : 1),
          previous[j + 1] + 1,
          row[j] + 1,
        ),
      );
    }
    previous = row;
  }
  return previous[hypothesis.length];
}

// Character error rate for Chinese, word error rate for English.
export function errorRate(reference, hypothesis, language) {
  const ref = units(reference, language);
  return { edits: edits(ref, units(hypothesis, language)), length: ref.length };
}

const han = /\p{Script=Han}/u;
const kana = /[\p{Script=Hiragana}\p{Script=Katakana}]/u;
const hangul = /\p{Script=Hangul}/u;
const latinWord = /\p{Script=Latin}[\p{L}\p{N}']*/gu;

// "zh" when Chinese characters outnumber Latin words, like
// `summary::dominant_script` in the app; "en" for Latin; null without letters.
export function languageOf(text) {
  const chars = [...text];
  const cjk = chars.filter((c) => han.test(c) || kana.test(c) || hangul.test(c)).length;
  const words = (text.match(latinWord) ?? []).length;
  if (cjk === 0 && words === 0) return null;
  return cjk >= words ? "zh" : "en";
}

const englishStopWords = new Set(
  `a an the and or but if of to in on at by for with from as is are was were be been
  being will would shall should can could may might must do does did have has had it its
  this that these those there here we us our you your they them their he she his her i me
  my not no so than then about into over after before up down out also just only all any
  both each more most other some such own same too very again until while when where who
  what which how why let lets let's`.split(/\s+/),
);
const chineseStopChars = new Set([..."的了是在和与及就也都而或把被给对从向为并这那个"]);

// Light stemming, so "updates" matches "update" and "agreed" matches "agree".
function stem(word) {
  return word.replace(/'s$/, "").replace(/(ing|ed|es|s)$/, "");
}

function contentUnits(text, language) {
  if (language === "zh") return units(text, "zh").filter((c) => !chineseStopChars.has(c));
  return units(text, "en")
    .filter((w) => !englishStopWords.has(w))
    .map(stem);
}

// Share of an item's content words (English) or characters (Chinese) that
// appear in the transcript. Names, dates, and actions invented by the model
// are not there.
export function support(item, transcript, language) {
  const wanted = contentUnits(item, language);
  if (wanted.length === 0) return 1;
  const said = new Set(contentUnits(transcript, language));
  return wanted.filter((unit) => said.has(unit)).length / wanted.length;
}

// An item counts as supported when at least this share of it was said.
export const SUPPORT_THRESHOLD = 0.6;

// How many checked items some output item covers, for information.
export function covered(checked, items, language) {
  return checked.filter((expected) =>
    items.some((item) => support(expected, item, language) >= 0.5),
  ).length;
}

// Error rates may rise by at most 2 points over the baseline.
export const ALLOWED_RISE = 0.02;

export function compareWithBaseline(current, baseline) {
  const failures = [];
  for (const [key, label] of [
    ["chinese_cer", "Chinese character error rate"],
    ["english_wer", "English word error rate"],
  ]) {
    if (current[key] > baseline[key] + ALLOWED_RISE + 1e-9) {
      failures.push(
        `${label} ${percent(current[key])} is more than 2 points above the baseline ${percent(baseline[key])}.`,
      );
    }
  }
  return failures;
}

export function percent(rate) {
  return `${(rate * 100).toFixed(1)}%`;
}
