---
name: anchovy-note-format
description: The shape of Anchovy's recording folders and note.md files. Use when writing or changing code that names recording folders, writes note.md or state.json, or builds note content.
---

# Anchovy note format

`note.md` is the user's file. Other tools, including Obsidian and future AI workflows, read it.

## Folder

- One folder per recording, named by start time: `yyyy-MM-dd-HHmm`. If the name is taken in the same minute, add `-2`, `-3`, and so on.
- Audio: `audio.wav` for High quality, `audio.m4a` for Small.
- App state lives in `.anchovy/state.json` inside the folder. Nothing else goes next to the note.

## note.md

Front matter keys, in this order: `date`, `duration`, `source` (`manual` or `meeting`), `inputs`, `audio`, `asr_model`, `summary_model`.

- `inputs` lists only what was actually recorded: `[microphone, computer audio]`, or `[microphone]` when computer audio was not allowed.
- `audio` is the real file name.

Then a `# yyyy-MM-dd HH:mm` title and these headings, in English, in this order: `## Summary`, `## Decisions`, `## Action items`, `## Transcript`, `## Audio`. Decisions and action items are bullet lists. The Audio section is a relative link to the audio file, so the note still finds it inside an Obsidian vault.

Body text stays in the language spoken in the meeting. Headings stay English.

## Writing

- Write to `.anchovy/note.tmp`, then atomically replace `note.md`. On any failure the old `note.md` is untouched and no partial note exists.
- The app writes `note.md` only when generating or regenerating. Regenerating asks the user first, because they may have edited the note.
- Audio is never deleted or rewritten when a note fails.
