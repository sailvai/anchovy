---
name: anchovy-privacy
description: Anchovy's local-only privacy rules. Use for any change that touches network access, external addresses, dependencies, entitlements, permissions, CSP, recording, transcripts, notes, model downloads, test fixtures, or eval samples.
---

# Anchovy privacy

Treat a conflict with these rules as a blocker. Say so in the pull request instead of working around it.

## Rules

1. Audio, transcripts, and notes never leave the Mac. No upload and no cloud fallback, not even as an option.
2. No account and no sign-in.
3. No analytics, crash reporting, advertising, or SDKs that phone home.
4. The app goes online for two things only: downloading models the user picked from the shipped list, and checking for updates.
5. Every external address in code or configuration is listed in `privacy/allowed-urls.txt`.
6. Every dependency and every model has a license in `privacy/deny.toml`.
7. A permission prompt says in one sentence what it is for. When a permission is denied, the app says what it cannot do. A recording without computer audio is labeled microphone only.
8. Nothing records without the user pressing Record or answering a prompt. No silent recording.
9. Test fixtures and eval samples contain only audio we recorded ourselves or audio licensed for redistribution. Never a real meeting.

## Where the rules are enforced

Skills can be skipped; these checks cannot:

- `npm run verify` runs `scripts/check-privacy.mjs` (addresses, tracker packages, CSP, `macOSPrivateApi`, npm and model licenses) and `cargo deny` (Rust licenses).
- `.claude/hooks/protect-files.sh` stops before edits to entitlements, `Info.plist`, Tauri config and capabilities, `privacy/`, and `models.json`, and waits for a person.
- `.github/CODEOWNERS` requires Joshua's review for the same files.

## When a change needs a protected file

1. Say which file and why in your reply, before editing.
2. Keep the change to the smallest possible entry (one address, one license, one entitlement).
3. Explain it in the pull request under "Protected files changed".
