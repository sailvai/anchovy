//! Turns a transcript into the note's summary, decisions, and action items.
//!
//! The summary model must answer with one JSON object and nothing else:
//! `{"summary": "...", "decisions": [...], "action_items": [...]}`, written
//! in the language of the meeting. An answer that is not exactly that is
//! asked for once more; a second bad answer is an error, and no note is
//! written.
//!
//! A transcript longer than one chunk is summarized chunk by chunk, and the
//! partial answers are merged by the same model.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{EngineError, Prompt, Summarizer};

/// What the note is built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub summary: String,
    pub decisions: Vec<String>,
    pub action_items: Vec<String>,
}

const GIB: u64 = 1 << 30;

/// Transcript tokens per chunk. A Mac with 16 GB or more holds a 24,000
/// token chunk next to the summary model; an 8 GB Mac gets 8,000.
pub fn chunk_tokens(memory_bytes: u64) -> usize {
    if memory_bytes >= 16 * GIB {
        24_000
    } else {
        8_000
    }
}

/// Room for the instructions around a chunk.
pub const PROMPT_TOKENS: usize = 1024;
/// The longest answer the model may give.
pub const ANSWER_TOKENS: usize = 2048;

/// Context the summary model needs for chunks of `chunk_tokens`.
pub fn context_tokens(chunk_tokens: usize) -> u32 {
    (chunk_tokens + PROMPT_TOKENS + ANSWER_TOKENS) as u32
}

/// The writing system most of a text uses, to check that the summary is in
/// the language of the meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// Chinese characters.
    Han,
    /// Japanese: kana, usually with kanji.
    Kana,
    /// Korean.
    Hangul,
    /// English, French, German, Spanish, and other Latin-script languages.
    Latin,
}

impl Script {
    fn name(self) -> &'static str {
        match self {
            Script::Han => "Chinese characters",
            Script::Kana => "Japanese",
            Script::Hangul => "Korean",
            Script::Latin => "Latin letters",
        }
    }
}

/// `None` when the text has no letters at all.
pub fn dominant_script(text: &str) -> Option<Script> {
    let (mut han, mut kana, mut hangul, mut latin_words) = (0usize, 0usize, 0usize, 0usize);
    let mut in_word = false;
    for c in text.chars() {
        match c as u32 {
            0x3040..=0x30FF => kana += 1,
            0xAC00..=0xD7AF => hangul += 1,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F => han += 1,
            _ => {}
        }
        let latin = c.is_alphabetic() && (c.is_ascii() || ('\u{C0}'..='\u{24F}').contains(&c));
        if latin && !in_word {
            latin_words += 1;
        }
        in_word = latin || (in_word && c.is_alphanumeric());
    }
    let cjk = han + kana + hangul;
    if cjk == 0 && latin_words == 0 {
        None
    } else if cjk < latin_words {
        Some(Script::Latin)
    } else if hangul > han && hangul >= kana {
        Some(Script::Hangul)
    } else if kana > 0 && kana * 4 >= han {
        Some(Script::Kana)
    } else {
        Some(Script::Han)
    }
}

/// The language to ask for: what the transcription model reported most
/// often, or a guess from the script.
pub fn language_name(reported: &[Option<String>], transcript: &str) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for name in reported.iter().flatten().map(|name| name.trim()) {
        if name.is_empty() || name.eq_ignore_ascii_case("none") {
            continue;
        }
        match counts.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    // The first one reported wins a tie.
    let mut most: Option<(&str, usize)> = None;
    for (name, count) in counts {
        if most.is_none_or(|(_, best)| count > best) {
            most = Some((name, count));
        }
    }
    if let Some((name, _)) = most {
        return Some(name.to_string());
    }
    match dominant_script(transcript)? {
        Script::Han => Some("Chinese".into()),
        Script::Kana => Some("Japanese".into()),
        Script::Hangul => Some("Korean".into()),
        // Many languages share Latin letters; the prompt then asks for the
        // language spoken in the meeting.
        Script::Latin => None,
    }
}

/// Why an answer was not accepted, or the engine error that stopped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryError {
    Engine(EngineError),
    /// Both attempts gave an answer that is not the JSON asked for.
    Invalid(String),
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryError::Engine(err) => write!(f, "{err}"),
            SummaryError::Invalid(why) => {
                write!(f, "The summary model did not return a valid note. {why}")
            }
        }
    }
}

impl std::error::Error for SummaryError {}

impl From<EngineError> for SummaryError {
    fn from(err: EngineError) -> Self {
        SummaryError::Engine(err)
    }
}

/// Checks one answer. `expected` is the transcript's script; the answer must
/// be written in it.
pub fn parse(answer: &str, expected: Option<Script>) -> Result<Summary, String> {
    let mut text = answer.trim();
    // Models sometimes wrap JSON in a Markdown code fence. The fence adds
    // nothing, so it is removed rather than asked for again.
    if let Some(inner) = text.strip_prefix("```") {
        let inner = inner.strip_prefix("json").unwrap_or(inner);
        if let Some(inner) = inner.trim_end().strip_suffix("```") {
            text = inner.trim();
        }
    }
    let mut summary: Summary = serde_json::from_str(text).map_err(|err| match err.classify() {
        serde_json::error::Category::Data => format!("The answer has the wrong shape: {err}."),
        _ => "The answer is not JSON.".to_string(),
    })?;
    let clean = |items: Vec<String>| -> Vec<String> {
        items
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect()
    };
    summary.summary = summary.summary.trim().to_string();
    summary.decisions = clean(summary.decisions);
    summary.action_items = clean(summary.action_items);
    if summary.summary.is_empty() {
        return Err("The summary is empty.".into());
    }
    let written = [
        summary.summary.as_str(),
        &summary.decisions.join(" "),
        &summary.action_items.join(" "),
    ]
    .join(" ");
    if let (Some(expected), Some(written)) = (expected, dominant_script(&written)) {
        if expected != written {
            return Err(format!(
                "The answer is written in {}, but the meeting is in {}.",
                written.name(),
                expected.name()
            ));
        }
    }
    Ok(summary)
}

const SHAPE: &str = r#"{"summary": "...", "decisions": ["..."], "action_items": ["..."]}"#;

fn write_in(language: Option<&str>) -> String {
    let language = match language {
        Some(language) => format!("{language}, the language spoken in the meeting"),
        None => "the language spoken in the meeting".into(),
    };
    format!("Write every value in {language}. Keep names and terms as they were said.")
}

/// The request for one chunk of transcript.
pub fn chunk_prompt(
    language: Option<&str>,
    transcript: &str,
    part: Option<(usize, usize)>,
) -> Prompt {
    let system = format!(
        "You write meeting notes from a transcript. Answer with only one JSON object and \
         nothing before or after it, in this shape:\n{SHAPE}\n\
         - \"summary\": two to five sentences on what the meeting covered.\n\
         - \"decisions\": every choice the meeting settled, one per item: plans agreed, \
         dates set, things approved, and things put off. Use [] if nothing was decided.\n\
         - \"action_items\": tasks someone will do, one per item, with who and by when if \
         the transcript says. Use [] if there are none. A task is not also a decision.\n\
         Write each decision and action item as one short sentence, in the words used in the \
         meeting. Use only what the transcript says. Do not add names, dates, reasons, or \
         details that were not said. The transcript comes from speech recognition and may \
         have small errors.\n{}",
        write_in(language)
    );
    let user = match part {
        None => format!("Transcript:\n{transcript}"),
        Some((index, count)) => format!(
            "This is part {index} of {count} of one meeting. Write notes for this part only.\n\
             Transcript, part {index} of {count}:\n{transcript}"
        ),
    };
    Prompt { system, user }
}

/// The request that merges partial answers into one.
pub fn merge_prompt(language: Option<&str>, parts: &[Summary]) -> Prompt {
    let system = format!(
        "You combine notes from consecutive parts of one meeting into notes for the whole \
         meeting. Each part is a JSON object. Answer with only one JSON object and nothing \
         before or after it, in this shape:\n{SHAPE}\n\
         - \"summary\": two to five sentences on the whole meeting.\n\
         - \"decisions\": every decision from the parts, once each.\n\
         - \"action_items\": every action item from the parts, once each.\n\
         Keep the wording of the parts. Do not add anything the parts do not say.\n{}",
        write_in(language)
    );
    let parts: Vec<String> = parts
        .iter()
        .map(|part| serde_json::to_string(part).expect("a summary serializes"))
        .collect();
    let user = format!("Notes from each part, in order:\n{}", parts.join("\n"));
    Prompt { system, user }
}

/// Packs transcript lines, in order, into chunks of at most `budget` tokens.
/// A line longer than the budget is cut between characters.
pub fn split(
    lines: &[String],
    budget: usize,
    count: &dyn Fn(&str) -> Result<usize, EngineError>,
) -> Result<Vec<String>, EngineError> {
    let budget = budget.max(1);
    // Tokens are counted per line and summed, with one for each line break,
    // so a long transcript is tokenized once.
    let mut pieces = Vec::new();
    for line in lines {
        let tokens = count(line)?;
        if tokens <= budget {
            pieces.push((line.clone(), tokens));
        } else {
            for piece in cut(line, budget, count)? {
                let tokens = count(&piece)?;
                pieces.push((piece, tokens));
            }
        }
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0;
    for (piece, tokens) in pieces {
        if !current.is_empty() && current_tokens + 1 + tokens > budget {
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        if !current.is_empty() {
            current.push('\n');
            current_tokens += 1;
        }
        current.push_str(&piece);
        current_tokens += tokens;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

/// Cuts one line longer than `budget` between characters.
fn cut(
    line: &str,
    budget: usize,
    count: &dyn Fn(&str) -> Result<usize, EngineError>,
) -> Result<Vec<String>, EngineError> {
    let mut pieces = Vec::new();
    let mut rest: Vec<char> = line.chars().collect();
    while !rest.is_empty() {
        let text: String = rest.iter().collect();
        let tokens = count(&text)?;
        if tokens <= budget {
            pieces.push(text);
            break;
        }
        // Guess from the average, then shrink until it fits.
        let mut take = (rest.len() * budget / tokens).clamp(1, rest.len());
        loop {
            let piece: String = rest[..take].iter().collect();
            if take == 1 || count(&piece)? <= budget {
                pieces.push(piece);
                break;
            }
            take = (take * 9 / 10).max(1);
        }
        rest.drain(..take);
    }
    Ok(pieces)
}

/// How far the summary has come: `done` of `total` model calls.
pub type OnPart<'a> = &'a mut dyn FnMut(usize, usize);

/// Summarizes transcript `lines` in chunks of `budget` tokens, merging the
/// partial answers when there is more than one.
pub fn summarize(
    model: &mut dyn Summarizer,
    lines: &[String],
    language: Option<&str>,
    budget: usize,
    on_part: OnPart,
) -> Result<Summary, SummaryError> {
    let script = dominant_script(&lines.join("\n"));
    let chunks = split(lines, budget, &|text| model.count_tokens(text))?;
    if chunks.is_empty() {
        return Err(SummaryError::Invalid("The transcript is empty.".into()));
    }
    let count = chunks.len();
    let mut done = 0;
    let mut total = count + usize::from(count > 1);
    on_part(done, total);
    let mut parts = Vec::with_capacity(count);
    for (index, chunk) in chunks.iter().enumerate() {
        let part = (count > 1).then_some((index + 1, count));
        parts.push(ask(model, &chunk_prompt(language, chunk, part), script)?);
        done += 1;
        on_part(done, total);
    }
    while parts.len() > 1 {
        let groups = group(model, parts, language, budget)?;
        let merges = groups.iter().filter(|group| group.len() > 1).count();
        total = done + merges + usize::from(groups.len() > 1);
        parts = Vec::with_capacity(groups.len());
        for group in groups {
            if group.len() == 1 {
                parts.extend(group);
                continue;
            }
            parts.push(ask(model, &merge_prompt(language, &group), script)?);
            done += 1;
            on_part(done, total);
        }
    }
    Ok(parts.pop().expect("at least one chunk"))
}

/// Groups partial answers so each merge fits the budget. A merge always
/// takes at least two, so every round gets shorter.
fn group(
    model: &dyn Summarizer,
    parts: Vec<Summary>,
    language: Option<&str>,
    budget: usize,
) -> Result<Vec<Vec<Summary>>, EngineError> {
    let mut groups: Vec<Vec<Summary>> = Vec::new();
    let mut current: Vec<Summary> = Vec::new();
    for part in parts {
        current.push(part);
        if current.len() > 2 {
            let tokens = model.count_tokens(&merge_prompt(language, &current).user)?;
            if tokens > budget {
                let last = current.pop().expect("just pushed");
                groups.push(std::mem::replace(&mut current, vec![last]));
            }
        }
    }
    groups.push(current);
    Ok(groups)
}

/// One request, asked once more if the answer is not valid.
fn ask(
    model: &mut dyn Summarizer,
    prompt: &Prompt,
    script: Option<Script>,
) -> Result<Summary, SummaryError> {
    let mut why = String::new();
    for attempt in 0..2 {
        match parse(&model.complete(prompt, attempt)?, script) {
            Ok(summary) => return Ok(summary),
            Err(err) => why = err,
        }
    }
    Err(SummaryError::Invalid(why))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Counts one token per character and answers from a script.
    struct Fake {
        answers: VecDeque<String>,
        prompts: Vec<(Prompt, u32)>,
    }

    impl Fake {
        fn new(answers: &[&str]) -> Self {
            Fake {
                answers: answers.iter().map(|a| a.to_string()).collect(),
                prompts: Vec::new(),
            }
        }
    }

    impl Summarizer for Fake {
        fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
            Ok(text.chars().count())
        }

        fn complete(&mut self, prompt: &Prompt, attempt: u32) -> Result<String, EngineError> {
            self.prompts.push((prompt.clone(), attempt));
            self.answers
                .pop_front()
                .ok_or_else(|| EngineError::Run("no more answers".into()))
        }
    }

    const EN: &str = r#"{"summary": "We planned the launch.", "decisions": ["Ship on Friday."], "action_items": ["Sam writes the notes."]}"#;
    const ZH: &str = r#"{"summary": "我们讨论了发布。", "decisions": ["周五发布。"], "action_items": ["王磊周三前改完。"]}"#;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn chunks_follow_the_memory_of_the_mac() {
        assert_eq!(chunk_tokens(24 * GIB), 24_000);
        assert_eq!(chunk_tokens(16 * GIB), 24_000);
        assert_eq!(chunk_tokens(8 * GIB), 8_000);
        assert_eq!(context_tokens(8_000), 8_000 + 1024 + 2048);
    }

    #[test]
    fn a_json_answer_is_accepted() {
        let summary = parse(EN, Some(Script::Latin)).unwrap();
        assert_eq!(
            summary,
            Summary {
                summary: "We planned the launch.".into(),
                decisions: vec!["Ship on Friday.".into()],
                action_items: vec!["Sam writes the notes.".into()],
            }
        );
        let empty = r#"{"summary": "Short call.", "decisions": [], "action_items": []}"#;
        assert!(parse(empty, Some(Script::Latin)).is_ok());
    }

    #[test]
    fn a_json_code_fence_around_the_answer_is_tolerated() {
        let fenced = format!("```json\n{EN}\n```");
        assert_eq!(
            parse(&fenced, Some(Script::Latin)),
            parse(EN, Some(Script::Latin))
        );
    }

    #[test]
    fn anything_but_the_three_keys_is_rejected() {
        let bad = [
            ("Here are the notes: {}", "The answer is not JSON"),
            (r#"{"summary": "x y z", "decisions": []}"#, "missing field"),
            (
                r#"{"summary": "x y z", "decisions": "none", "action_items": []}"#,
                "invalid type",
            ),
            (
                r#"{"summary": "x y z", "decisions": [], "action_items": [], "notes": ""}"#,
                "unknown field",
            ),
            (
                r#"{"summary": "  ", "decisions": [], "action_items": []}"#,
                "The summary is empty.",
            ),
            (&format!("{EN} And that is all."), "The answer is not JSON"),
        ];
        for (answer, why) in bad {
            let err = parse(answer, Some(Script::Latin)).unwrap_err();
            assert!(err.contains(why), "{answer}: {err}");
        }
    }

    #[test]
    fn empty_list_items_are_dropped_and_items_are_trimmed() {
        let answer =
            r#"{"summary": " Done. ", "decisions": [" Ship. ", ""], "action_items": ["  "]}"#;
        let summary = parse(answer, Some(Script::Latin)).unwrap();
        assert_eq!(summary.summary, "Done.");
        assert_eq!(summary.decisions, ["Ship."]);
        assert!(summary.action_items.is_empty());
    }

    #[test]
    fn the_answer_must_be_in_the_language_of_the_meeting() {
        assert!(parse(ZH, Some(Script::Han)).is_ok());
        let err = parse(EN, Some(Script::Han)).unwrap_err();
        assert_eq!(
            err,
            "The answer is written in Latin letters, but the meeting is in Chinese characters."
        );
        assert!(parse(ZH, Some(Script::Latin)).is_err());
        // English product names inside a Chinese note are fine.
        let mixed = r#"{"summary": "我们讨论了 Anchovy 的发布和 Qwen3 模型。", "decisions": [], "action_items": []}"#;
        assert!(parse(mixed, Some(Script::Han)).is_ok());
    }

    #[test]
    fn scripts_are_told_apart() {
        assert_eq!(dominant_script("我们周五发布 Anchovy。"), Some(Script::Han));
        assert_eq!(
            dominant_script("We ship 我们 on Friday."),
            Some(Script::Latin)
        );
        assert_eq!(
            dominant_script("金曜日にリリースします。"),
            Some(Script::Kana)
        );
        assert_eq!(
            dominant_script("금요일에 출시합니다."),
            Some(Script::Hangul)
        );
        assert_eq!(
            dominant_script("Nous livrons vendredi."),
            Some(Script::Latin)
        );
        assert_eq!(dominant_script("… 123 !"), None);
    }

    #[test]
    fn the_language_is_what_the_model_reported_most() {
        let reported = [
            Some("Chinese".to_string()),
            None,
            Some("English".to_string()),
            Some("Chinese".to_string()),
        ];
        assert_eq!(language_name(&reported, "").as_deref(), Some("Chinese"));
        assert_eq!(
            language_name(&[None], "我们周五发布。").as_deref(),
            Some("Chinese")
        );
        assert_eq!(
            language_name(&[None], "We ship on Friday.").as_deref(),
            None
        );
        // Qwen3-ASR says "None" when it heard no speech.
        assert_eq!(
            language_name(&[Some("None".into())], "金曜日にリリース").as_deref(),
            Some("Japanese")
        );
    }

    #[test]
    fn the_prompt_asks_for_only_json_in_the_meeting_language() {
        let prompt = chunk_prompt(Some("Chinese"), "周五发布。", None);
        assert!(prompt.system.contains("only one JSON object"));
        assert!(prompt.system.contains(r#""summary""#));
        assert!(prompt.system.contains(r#""decisions""#));
        assert!(prompt.system.contains(r#""action_items""#));
        assert!(prompt.system.contains("in Chinese"));
        assert!(prompt.user.contains("周五发布。"));

        let unknown = chunk_prompt(None, "Ship it.", Some((2, 3)));
        assert!(unknown
            .system
            .contains("in the language spoken in the meeting"));
        assert!(unknown.user.contains("part 2 of 3"));
    }

    #[test]
    fn lines_are_packed_into_chunks_in_order() {
        let count = |text: &str| Ok(text.chars().count());
        let chunks = split(&lines(&["aaaa", "bbbb", "cc", "dddddd", "e"]), 10, &count).unwrap();
        assert_eq!(chunks, ["aaaa\nbbbb", "cc\ndddddd", "e"]);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 10);
        }
        let long = split(&lines(&["我们周五发布新版本"]), 4, &count).unwrap();
        assert_eq!(long, ["我们周五", "发布新版", "本"]);
    }

    #[test]
    fn a_short_transcript_takes_one_request() {
        let mut model = Fake::new(&[EN]);
        let mut parts = Vec::new();
        let summary = summarize(
            &mut model,
            &lines(&["We ship on Friday."]),
            Some("English"),
            24_000,
            &mut |done, total| parts.push((done, total)),
        )
        .unwrap();
        assert_eq!(summary.decisions, ["Ship on Friday."]);
        assert_eq!(model.prompts.len(), 1);
        assert_eq!(model.prompts[0].1, 0);
        assert_eq!(parts, [(0, 1), (1, 1)]);
    }

    #[test]
    fn a_bad_answer_is_asked_for_once_more() {
        let mut model = Fake::new(&["Sure! Here is the summary.", EN]);
        let summary = summarize(
            &mut model,
            &lines(&["We ship on Friday."]),
            None,
            24_000,
            &mut |_, _| {},
        );
        assert!(summary.is_ok());
        let attempts: Vec<u32> = model.prompts.iter().map(|(_, a)| *a).collect();
        assert_eq!(attempts, [0, 1]);
        assert_eq!(model.prompts[0].0, model.prompts[1].0);
    }

    #[test]
    fn two_bad_answers_are_an_error() {
        let mut model = Fake::new(&["not json", "{}", EN]);
        let err = summarize(
            &mut model,
            &lines(&["We ship on Friday."]),
            None,
            24_000,
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, SummaryError::Invalid(_)), "{err:?}");
        assert!(err
            .to_string()
            .starts_with("The summary model did not return a valid note."));
        assert_eq!(model.prompts.len(), 2);
    }

    #[test]
    fn a_long_transcript_is_summarized_in_chunks_and_merged() {
        let part1 =
            r#"{"summary": "Part one.", "decisions": ["Ship on Friday."], "action_items": []}"#;
        let part2 = r#"{"summary": "Part two.", "decisions": [], "action_items": ["Sam tests."]}"#;
        let merged = r#"{"summary": "Both parts.", "decisions": ["Ship on Friday."], "action_items": ["Sam tests."]}"#;
        let mut model = Fake::new(&[part1, part2, merged]);
        let mut parts = Vec::new();
        let text = lines(&[&"a ".repeat(40), &"b ".repeat(40)]);

        let summary = summarize(&mut model, &text, None, 100, &mut |done, total| {
            parts.push((done, total))
        })
        .unwrap();

        assert_eq!(summary.summary, "Both parts.");
        assert_eq!(model.prompts.len(), 3);
        assert!(model.prompts[0].0.user.contains("part 1 of 2"));
        assert!(model.prompts[1].0.user.contains("part 2 of 2"));
        let merge = &model.prompts[2].0;
        assert!(merge.user.contains("Part one.") && merge.user.contains("Sam tests."));
        assert_eq!(parts, [(0, 3), (1, 3), (2, 3), (3, 3)]);
    }

    #[test]
    fn a_merge_is_checked_and_retried_like_any_answer() {
        let part = r#"{"summary": "Part.", "decisions": [], "action_items": []}"#;
        let mut model = Fake::new(&[part, part, "oops", "oops"]);
        let text = lines(&[&"a ".repeat(40), &"b ".repeat(40)]);
        let err = summarize(&mut model, &text, None, 100, &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, SummaryError::Invalid(_)));
    }

    #[test]
    fn merges_that_do_not_fit_one_chunk_are_merged_in_rounds() {
        let part = r#"{"summary": "Part.", "decisions": [], "action_items": []}"#;
        let parsed = parse(part, None).unwrap();
        let mut model = Fake::new(&[part; 20]);
        // The budget holds a merge of three partial answers but not four.
        let budget = model
            .count_tokens(
                &merge_prompt(None, &[parsed.clone(), parsed.clone(), parsed.clone()]).user,
            )
            .unwrap();
        let four = model
            .count_tokens(
                &merge_prompt(
                    None,
                    &[parsed.clone(), parsed.clone(), parsed.clone(), parsed],
                )
                .user,
            )
            .unwrap();
        assert!(four > budget);
        // Six lines, each more than half the budget: six chunks.
        let text: Vec<String> = (0..6)
            .map(|i| i.to_string().repeat(budget * 6 / 10))
            .collect();

        let summary = summarize(&mut model, &text, None, budget, &mut |_, _| {});

        assert!(summary.is_ok());
        // Six chunks, two merges of three, one final merge.
        assert_eq!(model.prompts.len(), 9);
    }
}
