# Anchovy

Sailvai Anchovy: local meeting notes for Apple Silicon Macs. Tauri 2 shell, React and TypeScript interface in `src/`, Rust core in `src-tauri/`.

## Before you start a step

Work follows a private plan. If `../sdlc/intent/anchovy/` exists, read `spec.md` and the matching step of `plan.md` there before changing anything. In an Orca worktree that path may not resolve; use `~/sailvai/sdlc/intent/anchovy/`. If you cannot read the plan, stop and ask. Do not guess.

Never copy the spec or plan into this repository. Pull requests name the step only, for example "Plan step 4".

## Product constraints

- Apple Silicon Macs only, macOS 14.4 or later.
- Interface text is English only. Product names are not translated.
- Recordings, transcripts, and notes never leave the Mac. No upload, no cloud fallback.
- No analytics, crash reporting, advertising, or third-party SDKs that phone home.
- The network is used only to download models from the shipped list and to check for updates. Every external address is in `privacy/allowed-urls.txt`.
- Do not enable Tauri's `macos-private-api`. Do not use libraries that depend on private macOS APIs.
- The app sandbox is on from the first build.

## Working rules

- Every change comes with tests. Write the failing test first.
- When fixing a bug, commit a test that reproduces it first, then fix without changing that test.
- Never delete tests, add skips, or loosen assertions to make a check pass.
- Run `npm run verify` before you say you are done, and paste the last lines of its output into the pull request.
- Recording changes also need `npm run verify:device`; model changes also need `npm run eval`.
- Files listed in `.github/CODEOWNERS` are protected. Editing them needs a person's approval.
- Keep system calls in thin `mac.rs` files and logic in modules that can be unit tested.
