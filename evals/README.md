# Model evaluation

`npm run eval` runs the app's own note pipeline with the shipped default models on every sample
here, on this Mac, and compares the result with `baseline.json`. It needs the models downloaded
(the app's Models screen, or `npm run eval -- --fetch`). It is never part of `npm run verify` or
CI.

## Samples

Each folder in `samples/` has:

- `transcript.txt`: the checked transcript. macOS's built-in voice reads it to make the audio, so
  it is exactly what was said. A sample with several speakers has `script.txt` instead: one turn
  per line, `Voice: text`, and the transcript is the turns' text. The voice is the name `say -v '?'`
  lists, such as `Flo (Chinese (China mainland))`. The eval stops if a voice is not installed,
  because `say` would otherwise read with a default voice without saying so.
- `sample.json`: the language (`zh`, `en`, or `mixed`), the voice when there is one, the
  background noise when there is some, the checked decisions and action items, worded as they
  were said, and `not_items`: things said in the meeting that must not come back as a decision or
  an action item. A not-item is a suggestion or proposal nobody agreed to, a question, an
  opinion, a status report, a problem description, or chit-chat such as "Fine with me.". Each is
  one sentence or clause copied word for word from the script. A suggestion the meeting later
  agreed to is a decision, not a not-item.

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

The other samples are made-up meetings of 35 seconds to about 5 minutes, each read by two to four
voices: 10 Chinese (`zh-*`), 10 English (`en-*`), and 3 short mixed ones (`mixed-sync` switches
language turn by turn, `mixed-bugs` within a turn, `mixed-launch` both). The Chinese ones use
Tingting and the mainland Chinese Eloquence voices (Flo, Reed, Eddy, and others), never the Taiwan
or Hong Kong voices, whose speech may be written in traditional characters. The Eloquence Chinese
voices misread Latin letters and English words, so their turns have none; Tingting reads the English
in Chinese and mixed samples. `zh-quarter-review`,
`en-quarter-review`, and `en-incident` are the long ones.

Some samples have background noise, set in `sample.json` as
`"noise": { "type": "pink", "snr_db": 20, "seed": 101 }`. `scripts/eval-audio.mjs` makes it
from the seed, so it is the same on every Mac, and adds it `snr_db` decibels below the speech's
level (root mean square over the whole clip, turn gaps included). `white` is a hiss, `pink` a room
or a fan, `brown` a low rumble like air conditioning. The samples use 20 dB (light) and 15 dB
(clearly audible).

| Sample                                                                                         | Noise        |
| ---------------------------------------------------------------------------------------------- | ------------ |
| `zh-design-review`, `zh-quarter-review`, `en-design-review`, `en-quarter-review`, `mixed-bugs` | pink, 20 dB  |
| `zh-support`, `en-support`                                                                     | white, 15 dB |
| `zh-budget`, `en-vendor`                                                                       | brown, 15 dB |
| `mixed-launch`                                                                                 | brown, 20 dB |

Synthetic speech is clean, even with the noise added, so error rates here are better than in real
meetings.

## What passes

Each sample runs twice: as the app would, and with small summary chunks so the chunk-and-merge
path runs on the real model too. Short samples use 120-token chunks; `zh-offsite`, `zh-standup`,
and `mixed-bugs` fit in one of those, so they set 60 in `sample.json`. `mixed-hour` uses 8,000, what
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

Two known limits of v0.1.0 are printed as warnings (`WARN … (known …)`) and counted under Totals
apart from the failures. They do not fail the run, in any sample. The warning totals should not grow
in a pull request without a reason in its description.

- Not-item warnings. A decision or action item matches one of the sample's not-items: something
  said that the meeting never agreed to or took on. The summary model lists status reports,
  problems, and questions as decisions or tasks. The support check cannot see this mistake, because
  the item was said. The fix planned for the next version is to make each item quote the transcript
  line it comes from. An item matches a not-item when at least 60% of the item's content units are
  in the not-item and at least 60% of the not-item's are in the item. A copy, shortened or lightly
  changed, matches both ways. A real decision often reuses most of the words of the problem it
  solves, or is mostly made of them, but not both, because it adds what was decided. A paraphrase
  of a not-item is not caught. A line that a real decision may repeat nearly word for word, such
  as the problem the decision fixes, cannot be a not-item: word overlap cannot tell the two apart.
- Mixed-language warnings. In a mixed sample, a decision or action item that is not supported. The
  summary model translates items between Chinese and English, and a faithful translation shares
  few words with what was said, so the support check cannot tell it from an invented item. Every
  other check still fails a mixed sample: valid JSON, the summary language, joins, chunking, the
  error rate against its baseline, and, for `mixed-hour`, time and memory.

How many checked decisions and action items the output covers is printed for information, and so
are the time and memory of the short samples; none of these fails
the run.

The first passing run writes `baseline.json`. A sample new to the baseline is added on its first
passing run. Changing it later needs a reviewed pull request.
