//! Long recordings are transcribed in fixed windows that overlap a little,
//! so a word cut at one window's edge is heard whole in the next. The words
//! heard twice in the overlap are then dropped where the texts join.
//!
//! The result is timed text: one segment per window, stamped with the time
//! its window starts. The models give no finer timing than that.

/// Seconds of audio per window. About 400 audio tokens for Qwen3-ASR, well
/// inside its context.
pub const WINDOW_SECONDS: f64 = 30.0;
/// Seconds each window shares with the one before it.
pub const OVERLAP_SECONDS: f64 = 3.0;

/// A span of samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: usize,
    pub end: usize,
}

/// Windows of `window` seconds, each starting `overlap` seconds before the
/// last one ends, covering `len` samples at `rate` Hz. The last window is
/// shorter. A window that would hold only audio already covered is left out.
pub fn plan(len: usize, rate: u32, window: f64, overlap: f64) -> Vec<Window> {
    let size = (window * f64::from(rate)).round() as usize;
    let shared = (overlap * f64::from(rate)).round() as usize;
    let step = size.saturating_sub(shared).max(1);
    let mut windows = Vec::new();
    let mut start = 0;
    while start < len && (start == 0 || start + shared < len) {
        windows.push(Window {
            start,
            end: (start + size).min(len),
        });
        start += step;
    }
    windows
}

/// Text heard in one window, with the time the window starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_seconds: f64,
    pub text: String,
}

impl Segment {
    /// `hh:mm:ss text`, the transcript line format in note.md.
    pub fn line(&self) -> String {
        let whole = self.start_seconds.max(0.0) as u64;
        format!(
            "{:02}:{:02}:{:02} {}",
            whole / 3600,
            whole / 60 % 60,
            whole % 60,
            self.text
        )
    }
}

/// Joins window texts into segments, dropping the words each window repeats
/// from the one before.
#[derive(Debug, Default)]
pub struct Joiner {
    segments: Vec<Segment>,
    /// Index of the window the last segment came from.
    last_window: Option<usize>,
}

impl Joiner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the text of window `index`, which starts at `start_seconds`.
    /// Windows must come in order.
    pub fn push(&mut self, index: usize, start_seconds: f64, text: &str) {
        let mut text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let next_to_last = self.last_window.is_some_and(|last| last + 1 == index);
        if let (true, Some(previous)) = (next_to_last, self.segments.last_mut()) {
            if let Some((keep, skip)) = overlap(&previous.text, &text) {
                previous.text.truncate(keep);
                text = text[skip..]
                    .trim_start_matches(|c: char| c.is_whitespace() || is_punctuation(c))
                    .to_string();
            }
        }
        self.last_window = Some(index);
        if !text.is_empty() {
            self.segments.push(Segment {
                start_seconds,
                text,
            });
        }
    }

    pub fn finish(self) -> Vec<Segment> {
        self.segments
    }
}

/// All the text, for the summary model and the evaluation.
pub fn plain_text(segments: &[Segment]) -> String {
    let mut out = String::new();
    for segment in segments {
        let spaced = matches!(
            (out.chars().last(), segment.text.chars().next()),
            (Some(a), Some(b)) if !is_cjk(a) && !is_cjk(b)
        );
        if spaced {
            out.push(' ');
        }
        out.push_str(&segment.text);
    }
    out
}

/// How many units at the end of one window and the start of the next are
/// searched for a shared run. Three seconds of fast speech fit easily.
const JOIN_SEARCH_UNITS: usize = 40;
/// A shared run needs at least two units and four letters, so a lone "the"
/// or "我们" is not taken for an overlap.
const MIN_RUN_UNITS: usize = 2;
const MIN_RUN_CHARS: usize = 4;

/// A word, or one CJK character, normalized, with where it ends.
#[derive(Debug)]
struct Unit {
    norm: String,
    /// Byte offset just past the unit.
    end: usize,
}

/// CJK scripts are written without spaces, so each character is a unit.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF     // Hiragana, Katakana
        | 0x3400..=0x4DBF   // CJK Extension A
        | 0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0xAC00..=0xD7AF   // Hangul syllables
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0x20000..=0x2FA1F // CJK Extensions B and later
    )
}

fn is_punctuation(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace()
}

fn units(text: &str) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut word: Option<usize> = None;
    let close = |units: &mut Vec<Unit>, start: usize, end: usize| {
        units.push(Unit {
            norm: text[start..end].to_lowercase(),
            end,
        });
    };
    for (at, c) in text.char_indices() {
        let in_word = (c.is_alphanumeric() && !is_cjk(c)) || (c == '\'' && word.is_some());
        match (in_word, word) {
            (true, None) => word = Some(at),
            (false, Some(start)) => {
                close(&mut units, start, at);
                word = None;
            }
            _ => {}
        }
        if is_cjk(c) {
            close(&mut units, at, at + c.len_utf8());
        }
    }
    if let Some(start) = word {
        close(&mut units, start, text.len());
    }
    units
}

/// Finds the longest run of units that ends one text and starts the next,
/// near their edges. Returns where to cut: keep `previous[..keep]` and
/// `next[skip..]`. Text after the run in `previous` was near its window's
/// edge, and `next` heard it whole, so it goes; so does `next`'s text
/// before the run.
fn overlap(previous: &str, next: &str) -> Option<(usize, usize)> {
    let a = units(previous);
    let b = units(next);
    let a = &a[a.len().saturating_sub(JOIN_SEARCH_UNITS)..];
    let b = &b[..b.len().min(JOIN_SEARCH_UNITS)];
    // (length, end in a, end in b) of the best run so far.
    let mut best: Option<(usize, usize, usize)> = None;
    // Classic longest-common-substring table, one row at a time.
    let mut row = vec![0usize; b.len() + 1];
    for (i, unit_a) in a.iter().enumerate() {
        let mut diagonal = 0;
        for (j, unit_b) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if unit_a.norm == unit_b.norm {
                diagonal + 1
            } else {
                0
            };
            diagonal = above;
            let len = row[j + 1];
            // Longer wins; on a tie, the run later in `previous`.
            if len > 0 && best.is_none_or(|(best_len, ..)| len >= best_len) {
                best = Some((len, i, j));
            }
        }
    }
    let (len, end_a, end_b) = best?;
    let chars: usize = a[end_a + 1 - len..=end_a]
        .iter()
        .map(|unit| unit.norm.chars().count())
        .sum();
    if len < MIN_RUN_UNITS || chars < MIN_RUN_CHARS {
        return None;
    }
    // Punctuation right after the run in `previous` stays only if `next`
    // has punctuation there too. Otherwise the model closed a sentence at
    // the window's edge that `next` shows going on.
    let next_punctuated = next[b[end_b].end..]
        .trim_start()
        .starts_with(is_punctuation);
    let keep = if next_punctuated {
        a[end_a].end
            + previous[a[end_a].end..]
                .char_indices()
                .find(|(_, c)| !is_punctuation(*c))
                .map_or(previous.len() - a[end_a].end, |(at, _)| at)
    } else {
        a[end_a].end
    };
    Some((keep, b[end_b].end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn secs(window: &Window) -> (f64, f64) {
        (
            window.start as f64 / RATE as f64,
            window.end as f64 / RATE as f64,
        )
    }

    fn joined(windows: &[&str]) -> Vec<String> {
        let mut joiner = Joiner::new();
        for (index, text) in windows.iter().enumerate() {
            joiner.push(index, index as f64 * 27.0, text);
        }
        joiner.finish().into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn long_audio_is_split_into_overlapping_windows() {
        let windows = plan(70 * RATE as usize, RATE, 30.0, 3.0);
        let spans: Vec<_> = windows.iter().map(secs).collect();
        assert_eq!(spans, [(0.0, 30.0), (27.0, 57.0), (54.0, 70.0)]);
    }

    #[test]
    fn short_audio_is_one_window() {
        assert_eq!(
            plan(10 * RATE as usize, RATE, 30.0, 3.0),
            [Window {
                start: 0,
                end: 10 * RATE as usize
            }]
        );
        assert_eq!(plan(30 * RATE as usize, RATE, 30.0, 3.0).len(), 1);
        assert!(plan(0, RATE, 30.0, 3.0).is_empty());
    }

    #[test]
    fn a_window_with_nothing_new_is_left_out() {
        // Exactly 30 s: a second window would start at 27 s and hear only
        // what the first already heard. At 30.5 s it hears half a second new.
        assert_eq!(plan(30 * RATE as usize, RATE, 30.0, 3.0).len(), 1);
        assert_eq!(plan(32 * RATE as usize, RATE, 30.0, 3.0).len(), 2);
        assert_eq!(
            plan(30 * RATE as usize + RATE as usize / 2, RATE, 30.0, 3.0).len(),
            2
        );
        let one_hour = plan(3600 * RATE as usize, RATE, 30.0, 3.0);
        assert_eq!(one_hour.len(), 134);
        assert_eq!(one_hour.last().unwrap().end, 3600 * RATE as usize);
    }

    #[test]
    fn repeated_words_at_a_join_are_dropped() {
        assert_eq!(
            joined(&[
                "We will ship on Friday and then",
                "on Friday and then test the build.",
            ]),
            ["We will ship on Friday and then", "test the build."]
        );
    }

    #[test]
    fn a_word_cut_at_the_edge_is_taken_from_the_next_window() {
        assert_eq!(
            joined(&[
                "Let's review the budg",
                "review the budget for next quarter."
            ]),
            ["Let's review the", "budget for next quarter."]
        );
    }

    #[test]
    fn chinese_joins_on_characters() {
        assert_eq!(
            joined(&["我们下周一开会讨论发布", "开会讨论发布日期的问题。"]),
            ["我们下周一开会讨论发布", "日期的问题。"]
        );
    }

    #[test]
    fn punctuation_and_case_do_not_stop_a_match() {
        assert_eq!(
            joined(&["Okay, ship it on Friday.", "on friday, then QA starts."]),
            ["Okay, ship it on Friday.", "then QA starts."]
        );
        assert_eq!(
            joined(&["大家好，今天开会。", "今天开会，先说进度。"]),
            ["大家好，今天开会。", "先说进度。"]
        );
    }

    #[test]
    fn a_full_stop_the_model_added_at_the_cut_is_dropped() {
        // The first window ended mid-sentence, and the model closed it.
        assert_eq!(
            joined(&[
                "Tom orders today. One more.",
                "one more thing: the offsite."
            ]),
            ["Tom orders today. One more", "thing: the offsite."]
        );
    }

    #[test]
    fn texts_without_a_shared_run_are_both_kept() {
        assert_eq!(
            joined(&["We agree.", "We need more time."]),
            ["We agree.", "We need more time."]
        );
    }

    #[test]
    fn a_window_that_only_repeats_adds_nothing() {
        assert_eq!(
            joined(&["and that is all for today", "all for today"]),
            ["and that is all for today"]
        );
    }

    #[test]
    fn silent_windows_are_skipped_and_do_not_join() {
        let mut joiner = Joiner::new();
        joiner.push(0, 0.0, "see you on Friday");
        joiner.push(1, 27.0, "   ");
        // Not next to window 0, so nothing is dropped.
        joiner.push(2, 54.0, "on Friday we ship");
        let texts: Vec<_> = joiner.finish().into_iter().map(|s| s.text).collect();
        assert_eq!(texts, ["see you on Friday", "on Friday we ship"]);
    }

    #[test]
    fn segments_keep_their_window_start_time() {
        let mut joiner = Joiner::new();
        joiner.push(0, 0.0, "Hello there.");
        joiner.push(1, 27.0, "Next point.");
        let segments = joiner.finish();
        assert_eq!(segments[1].start_seconds, 27.0);
        assert_eq!(segments[0].line(), "00:00:00 Hello there.");
        assert_eq!(segments[1].line(), "00:00:27 Next point.");
        let late = Segment {
            start_seconds: 3723.6,
            text: "Late.".into(),
        };
        assert_eq!(late.line(), "01:02:03 Late.");
    }

    #[test]
    fn plain_text_joins_segments_with_the_right_spacing() {
        let segments = |texts: &[&str]| -> Vec<Segment> {
            texts
                .iter()
                .map(|text| Segment {
                    start_seconds: 0.0,
                    text: text.to_string(),
                })
                .collect()
        };
        assert_eq!(
            plain_text(&segments(&["Ship it", "on Friday."])),
            "Ship it on Friday."
        );
        assert_eq!(
            plain_text(&segments(&["我们周五", "发布。"])),
            "我们周五发布。"
        );
    }
}
