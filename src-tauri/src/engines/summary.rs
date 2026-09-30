//! Turns a transcript into the note's summary, decisions, and action items.
//!
//! The summary model must answer with one JSON object and nothing else:
//! `{"summary": "...", "decisions": [...], "action_items": [...]}`, written
//! in the language of the meeting. In a meeting that switches languages,
//! each decision and action item stays in the language it was said in. An
//! answer that is not exactly that is asked for once more; a second bad
//! answer is an error, and no note is written.
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

/// How much of a text each script takes. CJK characters and Latin words
/// are counted alike.
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    han: usize,
    kana: usize,
    hangul: usize,
    latin_words: usize,
}

fn counts(text: &str) -> Counts {
    let mut counts = Counts::default();
    let mut in_word = false;
    for c in text.chars() {
        match c as u32 {
            0x3040..=0x30FF => counts.kana += 1,
            0xAC00..=0xD7AF => counts.hangul += 1,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F => {
                counts.han += 1
            }
            _ => {}
        }
        let latin = c.is_alphabetic() && (c.is_ascii() || ('\u{C0}'..='\u{24F}').contains(&c));
        if latin && !in_word {
            counts.latin_words += 1;
        }
        in_word = latin || (in_word && c.is_alphanumeric());
    }
    counts
}

impl Counts {
    fn dominant(&self) -> Option<Script> {
        let Counts {
            han,
            kana,
            hangul,
            latin_words,
        } = *self;
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

    /// The part of the text written in `script`. Japanese is kana with kanji.
    fn share(&self, script: Script) -> f64 {
        let total = self.han + self.kana + self.hangul + self.latin_words;
        let part = match script {
            Script::Han => self.han,
            Script::Kana if self.kana > 0 => self.kana + self.han,
            Script::Kana => 0,
            Script::Hangul => self.hangul,
            Script::Latin => self.latin_words,
        };
        if total == 0 {
            0.0
        } else {
            part as f64 / total as f64
        }
    }
}

/// `None` when the text has no letters at all.
pub fn dominant_script(text: &str) -> Option<Script> {
    counts(text).dominant()
}

/// A meeting can switch languages. A script at least this share of the
/// transcript counts as spoken, so the summary may be written in it.
const SPOKEN_SHARE: f64 = 0.2;

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

/// Checks one answer against a transcript: the answer must be JSON of the
/// right shape, written in the transcript's main script or in another script
/// that makes up a real part of it.
pub fn parse_for(answer: &str, transcript: &str) -> Result<Summary, String> {
    let summary = parse(answer, None)?;
    let spoken = counts(transcript);
    if let (Some(main), Some(written)) = (spoken.dominant(), written_script(&summary)) {
        if written != main && spoken.share(written) < SPOKEN_SHARE {
            return Err(wrong_script(written, main));
        }
    }
    Ok(summary)
}

fn written_script(summary: &Summary) -> Option<Script> {
    let written = [
        summary.summary.as_str(),
        &summary.decisions.join(" "),
        &summary.action_items.join(" "),
    ]
    .join(" ");
    dominant_script(&written)
}

fn wrong_script(written: Script, meeting: Script) -> String {
    format!(
        "The answer is written in {}, but the meeting is in {}.",
        written.name(),
        meeting.name()
    )
}

/// Checks one answer. `expected` is the script the answer must be written
/// in, if any.
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
    if let (Some(expected), Some(written)) = (expected, written_script(&summary)) {
        if expected != written {
            return Err(wrong_script(written, expected));
        }
    }
    Ok(summary)
}

const SHAPE: &str = r#"{"summary": "...", "decisions": ["..."], "action_items": ["..."]}"#;

/// The language to write in. The transcription model reports one language
/// even when a meeting switches, so a meeting with more than one `spoken`
/// script keeps each item in the script it was said in rather than
/// translating it.
fn write_in(language: Option<&str>, spoken: &[Script]) -> String {
    let names = "Keep names and terms as they were said.";
    if let [first, second, ..] = spoken {
        let (first, second) = (first.name(), second.name());
        let language = language.unwrap_or("the main language of the meeting");
        return format!(
            "The meeting uses more than one language. Write each decision and action item \
             in the language it was said in. Do not translate it. What was said in {first} \
             is written in {first}, and what was said in {second} is written in {second}, \
             even when the summary is in another language. Write the summary in {language}. \
             {names}"
        );
    }
    let language = match language {
        Some(language) => format!("{language}, the language spoken in the meeting"),
        None => "the language spoken in the meeting".into(),
    };
    format!("Write every value in {language}. {names}")
}

/// The scripts that each make up a real part of a transcript, most used
/// first. More than one means a mixed meeting, whose items may be in either.
pub fn spoken(transcript: &str) -> Vec<Script> {
    let Counts {
        han,
        kana,
        hangul,
        latin_words,
    } = counts(transcript);
    // Japanese is kana with kanji, so Chinese characters and kana count as one.
    let cjk = if kana > 0 && kana * 4 >= han {
        Script::Kana
    } else {
        Script::Han
    };
    let total = (han + kana + hangul + latin_words) as f64;
    let mut parts = [
        (cjk, han + kana),
        (Script::Hangul, hangul),
        (Script::Latin, latin_words),
    ];
    parts.sort_by_key(|&(_, part)| std::cmp::Reverse(part));
    parts
        .into_iter()
        .filter(|&(_, part)| part > 0 && part as f64 >= SPOKEN_SHARE * total)
        .map(|(script, _)| script)
        .collect()
}

/// The request for one chunk of transcript. `spoken` is what the whole
/// meeting used, from [`spoken`].
pub fn chunk_prompt(
    language: Option<&str>,
    spoken: &[Script],
    transcript: &str,
    part: Option<(usize, usize)>,
) -> Prompt {
    let system = format!(
        "You write meeting notes from a transcript. Answer with only one JSON object and \
         nothing before or after it, in this shape:\n{SHAPE}\n\
         - \"summary\": two to five sentences on what the meeting covered.\n\
         - \"decisions\": what the meeting agreed on, one per item: plans agreed, dates \
         set, things approved, and things put off. Use [] if nothing was decided.\n\
         - \"action_items\": tasks a person took on, one per item. Use [] if there are \
         none. A task is not also a decision.\n\
         A suggestion, a proposal, an opinion, or a question is not a decision unless the \
         meeting agreed to it, and not an action item unless someone took it on. A problem, \
         a status report, or an observation is neither. A possible follow-up nobody \
         mentioned is not an action item. Empty lists are fine.\n\
         Write each decision and action item as one short sentence in the words used in the \
         meeting: copy them from the transcript. Shorten, but do not rephrase or translate. \
         Keep the owner and the date when \
         they were said. If the owner was not said, leave the owner out. Never write \
         \"someone\" or any other placeholder. The transcript has no speaker names, so do not \
         guess who \"I\" is.\n\
         Use only what the transcript says. Do not add names, dates, reasons, or details that \
         were not said. The transcript comes from speech recognition and may have small \
         errors.\n{}",
        write_in(language, spoken)
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
pub fn merge_prompt(language: Option<&str>, spoken: &[Script], parts: &[Summary]) -> Prompt {
    let system = format!(
        "You combine notes from consecutive parts of one meeting into notes for the whole \
         meeting. Each part is a JSON object. Answer with only one JSON object and nothing \
         before or after it, in this shape:\n{SHAPE}\n\
         - \"summary\": two to five sentences on the whole meeting.\n\
         - \"decisions\": every decision from the parts.\n\
         - \"action_items\": every action item from the parts.\n\
         Copy each decision and action item exactly as a part wrote it, in the language it is \
         written in. Do not rewrite, combine, translate, or add items. Remove an item only \
         when it repeats another item with the same meaning.\n{}",
        write_in(language, spoken)
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
    let transcript = lines.join("\n");
    let chunks = split(lines, budget, &|text| model.count_tokens(text))?;
    if chunks.is_empty() {
        return Err(SummaryError::Invalid("The transcript is empty.".into()));
    }
    let spoken = spoken(&transcript);
    let count = chunks.len();
    let mut done = 0;
    let mut total = count + usize::from(count > 1);
    on_part(done, total);
    let mut parts = Vec::with_capacity(count);
    for (index, chunk) in chunks.iter().enumerate() {
        let part = (count > 1).then_some((index + 1, count));
        parts.push(ask(
            model,
            &chunk_prompt(language, &spoken, chunk, part),
            &transcript,
        )?);
        done += 1;
        on_part(done, total);
    }
    while parts.len() > 1 {
        let groups = group(model, parts, language, &spoken, budget)?;
        let merges = groups.iter().filter(|group| group.len() > 1).count();
        total = done + merges + usize::from(groups.len() > 1);
        parts = Vec::with_capacity(groups.len());
        for group in groups {
            if group.len() == 1 {
                parts.extend(group);
                continue;
            }
            parts.push(ask(
                model,
                &merge_prompt(language, &spoken, &group),
                &transcript,
            )?);
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
    spoken: &[Script],
    budget: usize,
) -> Result<Vec<Vec<Summary>>, EngineError> {
    let mut groups: Vec<Vec<Summary>> = Vec::new();
    let mut current: Vec<Summary> = Vec::new();
    for part in parts {
        current.push(part);
        if current.len() > 2 {
            let tokens = model.count_tokens(&merge_prompt(language, spoken, &current).user)?;
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
    transcript: &str,
) -> Result<Summary, SummaryError> {
    let mut why = String::new();
    for attempt in 0..2 {
        match parse_for(&model.complete(prompt, attempt)?, transcript) {
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
    fn a_mixed_meeting_may_be_summarized_in_any_language_it_really_used() {
        let mixed = "大家好，很高兴认识大家，我叫小王。I work remotely from Berlin, and I'm \
                     happy to join the team and learn from all of you.";
        assert!(parse_for(ZH, mixed).is_ok());
        assert!(parse_for(EN, mixed).is_ok());
        // A few English terms in a Chinese meeting do not make it English.
        let chinese = "我们讨论了 Anchovy 的发布和 Qwen3 模型，下周再测一次。";
        assert_eq!(
            parse_for(EN, chinese).unwrap_err(),
            "The answer is written in Latin letters, but the meeting is in Chinese characters."
        );
        assert!(parse_for(ZH, "We ship on Friday and test again next week.").is_err());
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
        let prompt = chunk_prompt(Some("Chinese"), &[], "周五发布。", None);
        assert!(prompt.system.contains("only one JSON object"));
        assert!(prompt.system.contains(r#""summary""#));
        assert!(prompt.system.contains(r#""decisions""#));
        assert!(prompt.system.contains(r#""action_items""#));
        assert!(prompt.system.contains("in Chinese"));
        assert!(prompt.user.contains("周五发布。"));

        let unknown = chunk_prompt(None, &[], "Ship it.", Some((2, 3)));
        assert!(unknown
            .system
            .contains("in the language spoken in the meeting"));
        assert!(unknown.user.contains("part 2 of 3"));
    }

    #[test]
    fn suggestions_and_questions_are_not_decisions_or_tasks() {
        let system = chunk_prompt(Some("English"), &[], "Ship it.", None).system;
        assert!(system.contains(
            "A suggestion, a proposal, an opinion, or a question is not a decision unless \
             the meeting agreed to it"
        ));
        assert!(system.contains("not an action item unless someone took it on"));
        assert!(system.contains("A possible follow-up nobody mentioned is not an action item"));
    }

    #[test]
    fn items_keep_the_words_owner_and_date_that_were_said() {
        let system = chunk_prompt(Some("English"), &[], "Ship it.", None).system;
        assert!(system.contains("Shorten, but do not rephrase or translate"));
        assert!(system.contains("Keep the owner and the date when they were said"));
        assert!(system.contains(r#"Never write "someone" or any other placeholder"#));
        assert!(system.contains(r#"do not guess who "I" is"#));
    }

    const MIXED: &str = "我们先用中文说一下这周的问题，格式会乱，下周一之前修好。\
                         On my side, users are asking for the date in the title, \
                         and I will change the format this week.";

    #[test]
    fn items_keep_the_language_they_were_said_in_when_the_meeting_is_mixed() {
        // The transcription model reports one language even for a mixed meeting.
        let system = chunk_prompt(Some("English"), &spoken(MIXED), MIXED, None).system;
        assert!(system.contains("The meeting uses more than one language."));
        assert!(system.contains("Write the summary in English."));
        assert!(system.contains(
            "Write each decision and action item in the language it was said in. \
             Do not translate it."
        ));
        assert!(!system.contains("Write every value in English"));
        assert!(system.contains(
            "What was said in Chinese characters is written in Chinese characters, and what \
             was said in Latin letters is written in Latin letters"
        ));
        assert_eq!(spoken(MIXED), [Script::Han, Script::Latin]);

        // A meeting in one language keeps one language for everything.
        let chinese =
            chunk_prompt(Some("Chinese"), &[], "我们周五发布，下周再测一次。", None).system;
        assert!(chinese.contains("Write every value in Chinese"));
        assert!(!chinese.contains("more than one language"));
        assert!(spoken("我们讨论了 Anchovy 的发布和 Qwen3 模型，下周再测一次。").len() < 2);
    }

    #[test]
    fn a_mixed_meeting_is_mixed_in_every_chunk_and_merge() {
        // Each chunk of a mixed meeting may be in one language; the prompts
        // follow the whole meeting.
        let part1 = r#"{"summary": "Part one.", "decisions": [], "action_items": []}"#;
        let part2 = r#"{"summary": "第二部分。", "decisions": [], "action_items": []}"#;
        let merged = r#"{"summary": "Both parts, 两部分。", "decisions": [], "action_items": []}"#;
        let mut model = Fake::new(&[part1, part2, merged]);
        let text = lines(&[
            &"We ship on Friday. ".repeat(4),
            &"我们周五发布新版本。".repeat(4),
        ]);
        summarize(&mut model, &text, Some("English"), 90, &mut |_, _| {}).unwrap();
        assert_eq!(model.prompts.len(), 3);
        for (prompt, _) in &model.prompts {
            assert!(
                prompt.system.contains("in the language it was said in"),
                "{}",
                prompt.system
            );
            assert!(!prompt.system.contains("Write every value in English"));
        }
    }

    #[test]
    fn the_merge_keeps_each_item_as_the_parts_wrote_it() {
        let part = parse(EN, None).unwrap();
        let system = merge_prompt(Some("English"), &[], &[part.clone(), part]).system;
        assert!(system.contains("Copy each decision and action item exactly as a part wrote it"));
        assert!(system.contains("in the language it is written in"));
        assert!(system.contains("Do not rewrite, combine, translate, or add items."));
        assert!(system.contains("Remove an item only when it repeats another item"));
    }

    /// More tokens than the summary model's tokenizer gives: every mark and
    /// CJK character is a token, and a word is one token per six letters.
    fn at_most_tokens(text: &str) -> usize {
        let mut tokens = 0;
        let mut word: usize = 0;
        for c in text.chars().chain([' ']) {
            if c.is_ascii_alphanumeric() {
                word += 1;
                continue;
            }
            tokens += word.div_ceil(6);
            word = 0;
            if !c.is_whitespace() {
                tokens += 1;
            }
        }
        tokens
    }

    #[test]
    fn the_instructions_fit_the_room_left_for_them() {
        assert_eq!(at_most_tokens("Ship it, Sam."), 5);
        assert_eq!(at_most_tokens("Keep wording 周五"), 5);
        // The chat template adds a few tokens around each message.
        let template = 32;
        let room = PROMPT_TOKENS * 3 / 4;
        let prompts = [
            chunk_prompt(Some("English"), &[], "", Some((10, 12))),
            chunk_prompt(
                Some("English"),
                &[Script::Han, Script::Latin],
                "",
                Some((10, 12)),
            ),
            chunk_prompt(None, &[Script::Han, Script::Latin], "", None),
            merge_prompt(Some("English"), &[], &[]),
            merge_prompt(Some("English"), &[Script::Han, Script::Latin], &[]),
        ];
        for prompt in prompts {
            let tokens = at_most_tokens(&prompt.system) + at_most_tokens(&prompt.user) + template;
            assert!(tokens <= room, "{tokens} > {room}: {}", prompt.system);
        }
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
                &merge_prompt(None, &[], &[parsed.clone(), parsed.clone(), parsed.clone()]).user,
            )
            .unwrap();
        let four = model
            .count_tokens(
                &merge_prompt(
                    None,
                    &[],
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
