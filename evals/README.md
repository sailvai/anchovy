# Model evaluation

`npm run eval` runs the app's own note pipeline with the shipped default models on every sample
here, on this Mac, and compares the result with `baseline.json`. It needs the models downloaded
(the app's Models screen, or `npm run eval -- --fetch`). It is never part of `npm run verify` or
CI.

## Samples

Each folder in `samples/` has:

- `transcript.txt`: the checked transcript. macOS's built-in voice reads it to make the audio, so
  it is exactly what was said. A sample with several speakers has `script.txt` instead: one turn
  per line, `Voice: text`, and the transcript is the turns' text.
- `sample.json`: the language (`zh`, `en`, or `mixed`), the voice when there is one, and the
  checked decisions and action items, worded as they were said.

The audio is synthetic speech made on the Mac at eval time with `say` and `afconvert`, as 48 kHz
16-bit mono WAV, the app's recording format (`scripts/eval-audio.mjs`). Turns are joined with 0.6
seconds of silence. It is not committed. There is no real meeting here, and there must never be
one.

`zh-weekly` and `en-weekly` are the scripts from `spikes/asr/clips/`. `zh-product` and
`en-planning` are longer, so each crosses two window joins.

`mixed-hour` is a made-up planning meeting of about an hour: Chinese stretches read by Tingting,
English stretches read by Samantha and Daniel, and stretches that mix the two, both within one
turn and turn by turn. It has eight decisions and eight action items across all three kinds of
stretch. Making its audio takes a few minutes, and running it takes several more. Run it alone
with `npm run eval -- --sample mixed-hour`.

Synthetic speech is clean, so error rates here are better than in real meetings.

## What passes

Each sample runs twice: as the app would, and with small summary chunks so the chunk-and-merge
path runs on the real model too. Short samples use 120-token chunks. `mixed-hour` uses 8,000, what
an 8 GB Mac gets; on a Mac with 16 GB or more its transcript fits in one chunk.

- Chinese character error rate and English word error rate stay within 2 points of the baseline.
- The `mixed-hour` error rate, scored by Chinese characters and English words together, stays
  within 2 points of its own baseline.
- Every summary is valid JSON (the pipeline retries once; the run shows how many answers it
  took).
- Every decision and action item is supported by the transcript: at least 60% of its content
  words (English) or characters (Chinese) were said in one passage of 400 content units, a few
  minutes of speech. Stop words do not count. Over an hour almost every common character is said
  somewhere, so the passage matters.
- Every summary is written in the language spoken. A mixed meeting may be summarized in either.
- The small-chunk run was summarized in two or more chunks and merged.
- No window join loses or repeats 3 or more units (characters or words) against the transcript.
  Smaller counts are printed: a number heard as digits shifts a count by one or two.
- For `mixed-hour` (`"timed": true`), transcription time, summary time, and peak memory are no
  more than 20% over its baseline.

How many checked decisions and action items the output covers is printed for information, and so
are the time and memory of the short samples; none of these fails
the run.

The first passing run writes `baseline.json`. A sample new to the baseline is added on its first
passing run. Changing it later needs a reviewed pull request.
