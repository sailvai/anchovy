---
name: anchovy-models
description: Rules for Anchovy's shipped model list. Use when adding, changing, or removing an entry in src-tauri/resources/models.json, changing a default model, or touching model download, verification, or engine code.
---

# Anchovy models

Users pick models only from the list that ships with the app. They never type a URL, choose a weights folder, or enter a launch command.

## Every entry

- `id`, `role` (`transcribe` or `summarize`), `engine`, `display_name`
- `languages` it is good at, shown in the interface
- `files`: for each file, the download URL, a pinned revision (never a moving branch), `sha256`, and size
- `min_ram_gb`, `license`, and `default`

## Before adding an entry

1. Confirm the license from the model's own source and quote it in the pull request. It must be in `privacy/deny.toml`.
2. Prefer official publishers or ggml-org repositories. Pin a revision and record every file's `sha256`.
3. Add the download host to `privacy/allowed-urls.txt` if it is new.
4. Add a line to `THIRD_PARTY_NOTICES.md`.
5. Run `npm run eval` on a real Mac when the entry is or could become a default, and paste the results.

`models.json`, the allowlists, and `evals/baseline.json` are protected. The hook will stop and wait for a person; say why the change is needed.

## Engines

Code outside `src-tauri/src/engines/` depends only on the `Transcriber` and `Summarizer` traits, never on a specific engine. A new engine implements both traits and gets its own `engine` value.
