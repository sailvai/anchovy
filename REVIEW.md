# Review rules

Rules for AI review of pull requests in this repository. Human reviewers can use them too.

Review the diff in three passes. Label each finding **Important** or **Nit**. Any Important finding blocks the merge.

## 1. Bugs and logic

- Wrong results, crashes, panics, unhandled errors, race conditions, resource leaks.
- State changes that skip or break the allowed recording states.
- A note written partly, or an existing `note.md` overwritten without the user confirming.
- Tests that do not test what their name says, or that pass whether or not the code works.

## 2. Security and privacy

- Any new external address, network call, or dependency that could send data off the Mac.
- Analytics, crash reporting, advertising, or other SDKs that phone home.
- Changes to entitlements, `Info.plist`, CSP, Tauri capabilities, the privacy allowlists, or `models.json` without a stated reason.
- Recording that can start without an explicit user action or answer.
- Secrets, real meeting audio, or real transcripts in the diff.

## 3. The step and product constraints

- The pull request names its plan step, and the change stays inside that step.
- Interface text is English only.
- Apple Silicon only; no `macos-private-api`.
- Tests were added or changed with the code, and none were deleted, skipped, or loosened without a written reason.
- The `npm run verify` output is in the pull request.

Do not report style that Prettier, ESLint, rustfmt, or Clippy already enforce.
