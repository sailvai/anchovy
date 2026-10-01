// Scoring for `npm run eval`: transcription error rates, whether each
// decision and action item is supported by what was said, the language of a
// summary, and the comparison with evals/baseline.json.

const cjkScripts = "\\p{Script=Han}\\p{Script=Hiragana}\\p{Script=Katakana}\\p{Script=Hangul}";
const mixedUnit = new RegExp(`[${cjkScripts}]|[^\\s${cjkScripts}]+`, "gu");

// Chinese is scored by characters, English by words, and a meeting that
// mixes them ("mixed") by both. Punctuation and case are ignored.
export function units(text, language) {
  const clean = text.toLowerCase().replace(/[^\p{L}\p{N}']+/gu, " ");
  if (language === "zh") return [...clean.replace(/\s+/g, "")];
  if (language === "mixed") return clean.match(mixedUnit) ?? [];
  return clean.split(/\s+/).filter(Boolean);
}

// Edit distance between two unit lists.
export function edits(reference, hypothesis) {
  let previous = Int32Array.from({ length: hypothesis.length + 1 }, (_, j) => j);
  let row = new Int32Array(hypothesis.length + 1);
  for (let i = 0; i < reference.length; i++) {
    row[0] = i + 1;
    for (let j = 0; j < hypothesis.length; j++) {
      row[j + 1] = Math.min(
        previous[j] + (reference[i] === hypothesis[j] ? 0 : 1),
        previous[j + 1] + 1,
        row[j] + 1,
      );
    }
    [previous, row] = [row, previous];
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
  return units(text, language)
    .filter((unit) => !englishStopWords.has(unit) && !chineseStopChars.has(unit))
    .map(stem);
}

// An item must be said in one place: one passage of this many content units,
// a few minutes of speech. Over an hour almost every common word and
// character is said somewhere, so the whole transcript would support anything.
export const PASSAGE_UNITS = 400;

// Share of an item's content words (English) or characters (Chinese) that
// appear in the passage of the transcript that has most of them. Names,
// dates, and actions invented by the model are not there.
export function support(item, transcript, language) {
  const wanted = contentUnits(item, language);
  if (wanted.length === 0) return 1;
  const said = contentUnits(transcript, language);
  const step = PASSAGE_UNITS / 4;
  let best = 0;
  for (let start = 0; start === 0 || start + PASSAGE_UNITS - step < said.length; start += step) {
    const passage = new Set(said.slice(start, start + PASSAGE_UNITS));
    best = Math.max(best, wanted.filter((unit) => passage.has(unit)).length / wanted.length);
  }
  return best;
}

// An item counts as supported when at least this share of it was said.
export const SUPPORT_THRESHOLD = 0.6;

// Share of `text`'s content units that are also in `other`.
export function overlap(text, other, language) {
  const wanted = contentUnits(text, language);
  if (wanted.length === 0) return 1;
  const there = new Set(contentUnits(other, language));
  return wanted.filter((unit) => there.has(unit)).length / wanted.length;
}

// An item is a not-item (something said that the meeting never agreed to or
// took on) when at least this share of each one's content units is in the
// other. A copy, shortened or lightly changed, matches both ways. A real
// decision often reuses most of the words of the problem it solves, or is
// mostly made of them, but not both, because it adds what was decided.
export const NOT_ITEM_MATCH = 0.6;

export function isNotItem(item, notItem, language) {
  return (
    overlap(item, notItem, language) >= NOT_ITEM_MATCH &&
    overlap(notItem, item, language) >= NOT_ITEM_MATCH
  );
}

const notKind = { decision: "Not a decision", "action item": "Not an action item" };

// Judges a summary's decisions and action items (`{ kind, text }`) against
// a sample. Each must be supported by the transcript and must not be one of
// the sample's not-items. In a mixed sample an unsupported item is a
// warning instead: the model translates items between Chinese and English,
// and a faithful translation fails a word-overlap check.
export function judgeItems(items, sample) {
  const problems = [];
  const warnings = [];
  const judged = items.map((item) => {
    const notItem = (sample.not_items ?? []).find((line) =>
      isNotItem(item.text, line, sample.language),
    );
    if (notItem) {
      problems.push(
        `${notKind[item.kind]} (said in the meeting, never agreed or taken on): "${item.text}" matches "${notItem}"`,
      );
    }
    const said = support(item.text, sample.transcript, sample.language);
    if (said < SUPPORT_THRESHOLD) {
      const unsupported = `Unsupported ${item.kind} (${percent(said)} said): ${item.text}`;
      if (sample.language === "mixed") warnings.push(`${unsupported} (known mixed-language limit)`);
      else problems.push(unsupported);
    }
    return { ...item, support: said, notItem };
  });
  return { items: judged, problems, warnings };
}

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

// A meeting in both languages is scored by characters and words together
// and held to its own baseline, with the same 2 points.
export function compareSampleError(id, rate, baseline) {
  if (rate > baseline.error_rate + ALLOWED_RISE + 1e-9) {
    return [
      `${id}: error rate ${percent(rate)} is more than 2 points above the baseline ${percent(baseline.error_rate)}.`,
    ];
  }
  return [];
}

// Time and memory for the hour may be at most 20% worse than the baseline.
export const ALLOWED_COST = 1.2;

export function compareCost(id, current, baseline) {
  const failures = [];
  for (const [value, base, what] of [
    [current.seconds.transcribe, baseline.seconds.transcribe, "transcription"],
    [current.seconds.summarize, baseline.seconds.summarize, "summary"],
  ]) {
    if (value > base * ALLOWED_COST) {
      failures.push(
        `${id}: ${what} took ${value.toFixed(1)} s, more than 20% over the baseline ${base.toFixed(1)} s.`,
      );
    }
  }
  const peak = current.memory.peak_footprint;
  const basePeak = baseline.memory.peak_footprint;
  if (peak > basePeak * ALLOWED_COST) {
    failures.push(
      `${id}: peak memory ${megabytes(peak)} is more than 20% over the baseline ${megabytes(basePeak)}.`,
    );
  }
  return failures;
}

export const megabytes = (bytes) => (bytes == null ? "-" : `${(bytes / 1e6).toFixed(0)} MB`);

// First answers to chunk and merge requests; retries are not counted.
export function summaryCalls(answers) {
  const first = answers.filter((answer) => answer.attempt === 0);
  return {
    chunks: first.filter((answer) => answer.kind === "chunk").length,
    merges: first.filter((answer) => answer.kind === "merge").length,
  };
}

// Units on each side of a join that are checked. A missed overlap repeats
// about three seconds of speech, well inside this.
export const JOIN_REACH = 20;
// Reference units around two windows searched for what they heard.
const REGION_MARGIN = 60;

// A join that loses or repeats this many units fails; smaller counts are
// printed. A number heard as digits ("二十四" as "24") shifts a count by one
// or two without anything lost, while a missed overlap repeats about three
// seconds of speech.
export const JOIN_LIMIT = 3;

// For each place two segments meet, how many content units (as in
// `support`) the joiner lost or repeated there. `heard` is the text the model wrote for each window, before
// the joins. Only what the join changed counts: units that one of the two
// windows heard but the joined text no longer has are lost, and units the
// joined text has beyond the model's own extra words are repeated. Words the
// model dropped, misheard, or made up inside a window are transcription
// errors, not join errors.
export function checkJoins(reference, segments, heard, language) {
  // Stop words are left out: they match almost anywhere, so a window
  // aligned on its own would place them differently than the joined text.
  const ref = contentUnits(reference, language);
  const segUnits = segments.map((segment) => contentUnits(segment.text, language));
  const raw = rawWindows(segments, heard).map((text, k) =>
    text == null ? segUnits[k] : contentUnits(text, language),
  );
  const hyp = segUnits.flat();
  const offsets = [0];
  for (const unitsOf of segUnits) offsets.push(offsets.at(-1) + unitsOf.length);
  const refOf = new Int32Array(hyp.length).fill(-1);
  for (const [i, j] of matches(ref, hyp)) refOf[j] = i;

  const joins = [];
  for (let k = 1; k < segments.length; k++) {
    const placed = [...refOf.subarray(offsets[k - 1], offsets[k + 1])].filter((i) => i >= 0);
    const join = { start_seconds: segments[k].start_seconds, lost: 0, repeated: 0 };
    joins.push(join);
    if (placed.length === 0) continue;
    const lo = Math.max(0, Math.min(...placed) - REGION_MARGIN);
    const region = ref.slice(lo, Math.max(...placed) + REGION_MARGIN + 1);
    const heardAt = (unitsOf) => matches(region, unitsOf);

    // Lost: heard by either window, gone from the joined text around it.
    const refIn = (pairs) => new Set(pairs.map(([i]) => i));
    const [a, b] = [refIn(heardAt(raw[k - 1])), refIn(heardAt(raw[k]))];
    const kept = refIn(heardAt(segUnits.slice(Math.max(0, k - 2), k + 2).flat()));
    if (a.size && b.size) {
      const lastA = Math.max(...a);
      const firstB = Math.min(...b);
      const from = Math.min(lastA, firstB) - JOIN_REACH;
      const to = Math.max(lastA, firstB) + JOIN_REACH;
      for (const i of new Set([...a, ...b])) if (i >= from && i <= to && !kept.has(i)) join.lost++;
    }

    // Repeated: extra units at the join, less the model's own extras there.
    const before = segUnits[k - 1];
    const after = segUnits[k];
    const joined = [...before, ...after];
    const matchedJoined = new Set(heardAt(joined).map(([, j]) => j));
    let extra = 0;
    for (let j = before.length - JOIN_REACH; j < before.length + JOIN_REACH; j++) {
      if (j >= 0 && j < joined.length && !matchedJoined.has(j)) extra++;
    }
    const own = (window, part, atEnd) => {
      const start = indexOfRun(window, part);
      if (start < 0) return 0;
      const reach = Math.min(JOIN_REACH, part.length);
      const from = atEnd ? start + part.length - reach : start;
      const matched = new Set(heardAt(window).map(([, j]) => j));
      let count = 0;
      for (let j = from; j < from + reach; j++) if (!matched.has(j)) count++;
      return count;
    };
    join.repeated = Math.max(0, extra - own(raw[k - 1], before, true) - own(raw[k], after, false));
  }
  return joins;
}

// The window text each segment was cut from, or null when it is not found.
// A segment is a stretch of its window's text, and windows come in order.
function rawWindows(segments, heard) {
  let next = 0;
  return segments.map((segment) => {
    const text = segment.text.trim();
    for (let h = next; h < heard.length; h++) {
      if (heard[h].includes(text)) {
        next = h + 1;
        return heard[h];
      }
    }
    return null;
  });
}

function indexOfRun(list, run) {
  outer: for (let start = 0; start + run.length <= list.length; start++) {
    for (let j = 0; j < run.length; j++) if (list[start + j] !== run[j]) continue outer;
    return start;
  }
  return -1;
}

// The longest common subsequence of `a` and `b` (Myers), as index pairs
// [i, j] with a[i] === b[j], in order.
export function matches(a, b) {
  const n = a.length;
  const m = b.length;
  const offset = n + m + 1;
  const v = new Int32Array(2 * offset + 1);
  const trace = [];
  let steps = 0;
  search: for (let d = 0; d <= n + m; d++) {
    trace.push(v.slice(offset - d, offset + d + 1));
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])
          ? v[offset + k + 1]
          : v[offset + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) {
        steps = d;
        break search;
      }
    }
  }
  const pairs = [];
  let x = n;
  let y = m;
  for (let d = steps; d > 0; d--) {
    const before = trace[d];
    const at = (k) => before[k + d];
    const k = x - y;
    const previousK = k === -d || (k !== d && at(k - 1) < at(k + 1)) ? k + 1 : k - 1;
    const previousX = at(previousK);
    const previousY = previousX - previousK;
    while (x > previousX && y > previousY) pairs.push([--x, --y]);
    x = previousX;
    y = previousY;
  }
  while (x > 0 && y > 0) pairs.push([--x, --y]);
  return pairs.reverse();
}

export function percent(rate) {
  return `${(rate * 100).toFixed(1)}%`;
}
