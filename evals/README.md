# Model evaluation

`npm run eval` runs the app's own note pipeline with the shipped default models on every sample
here, on this Mac, and compares the result with `baseline.json`. It needs the models downloaded
(the app's Models screen, or `npm run eval -- --fetch`). It is never part of `npm run verify` or
CI.

## Samples

Each folder in `samples/` has:

- `transcript.txt`: the checked transcript. macOS's built-in voice reads it to make the audio, so
  it is exactly what was said.
- `sample.json`: the language (`zh` or `en`), the voice, and the checked decisions and action
  items.

The audio is synthetic speech made on the Mac at eval time with `say` and `afconvert`, as 48 kHz
16-bit mono WAV, the app's recording format. It is not committed. There is no real meeting here,
and there must never be one.

`zh-weekly` and `en-weekly` are the scripts from `spikes/asr/clips/`. `zh-product` and
`en-planning` are longer, so each crosses two window joins.

Synthetic speech is clean, so error rates here are better than in real meetings.

## What passes

Each sample runs twice: as the app would, and with 120-token summary chunks so the
chunk-and-merge path runs on the real model too.

- Chinese character error rate and English word error rate stay within 2 points of the baseline.
- Every summary is valid JSON (the pipeline retries once; the run shows how many answers it
  took).
- Every decision and action item is supported by the transcript: at least 60% of its content
  words (English) or characters (Chinese) were said. Stop words do not count.
- Every summary is written in the language spoken.

How many checked decisions and action items the output covers, the time per step, and the
memory use are printed for information and saved in the baseline, but they do not fail the run.

The first passing run writes `baseline.json`. Changing it later needs a reviewed pull request.
