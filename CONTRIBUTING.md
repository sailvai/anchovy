# Contributing to Anchovy

Apple Silicon only. macOS 14.4 or later.

## Tools

Versions were checked on Joshua's Mac on 2026-09-27 (macOS 26.6, arm64).

| Tool    | Version                         | How to install                                                                                            |
| ------- | ------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Rust    | 1.98.1, with rustfmt and clippy | [rustup](https://rustup.rs), stable. `rust-toolchain.toml` selects 1.98.1 when you enter this repository. |
| Node.js | 22.23.3                         | Homebrew `node@22`, or nvm / fnm. `.node-version` records the version.                                    |
| npm     | 10.9.9                          | Comes with that Node.js.                                                                                  |
| CMake   | 4.4.3                           | `brew install cmake`                                                                                      |
| Xcode   | 27.0                            | Mac App Store, plus the command line tools                                                                |

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile default
brew install node@22 cmake
```

Add this to `~/.zshrc`, then open a new terminal. Homebrew does not put `node@22` on `PATH` by itself.

```sh
. "$HOME/.cargo/env"
export PATH="/opt/homebrew/opt/node@22/bin:$PATH"
```

Do not install the Tauri CLI globally. It will be a development dependency of this repository.

Put the three Sailvai repositories next to each other:

```text
~/sailvai/
  anchovy/
  sdlc/
  website/
```

Set the commit author inside each repository. Use your own name and GitHub noreply email. Do not change the global git config.

Two more tools, used by `npm run verify`:

```sh
cargo install --locked cargo-deny   # Rust license check; checked with 0.20.2
npx playwright install chromium     # browser for screenshot tests, after npm install
```

## Run the app

```sh
npm install
npm run tauri dev
```

The Anchovy window opens. It follows the macOS appearance, so switch System Settings > Appearance to check light and dark. Interface changes reload live; Rust changes rebuild the app.

`npm run dev` serves only the interface at `http://localhost:1420`. Rust commands do not work there.

## Test

| Command                      | What it checks                                   | Where           |
| ---------------------------- | ------------------------------------------------ | --------------- |
| `npm run verify`             | Everything below, then an unsigned `tauri build` | Your Mac and CI |
| `npm test`                   | Interface tests (Vitest)                         | Your Mac and CI |
| `npm run test:ui`            | Screenshot tests (Playwright), light and dark    | Your Mac and CI |
| `cargo test` in `src-tauri/` | Rust tests                                       | Your Mac and CI |
| `npm run check:privacy`      | External addresses, tracker SDKs, CSP, licenses  | Your Mac and CI |
| `npm run verify:device`      | Recording on real audio hardware                 | Your Mac only   |
| `npm run eval`               | Model quality against the baseline               | Your Mac only   |

`npm run verify` runs format, lint, type, and build checks, Vitest, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo deny`, the privacy and English-only text checks, the agent hook tests, the screenshot tests, and an unsigned app build. It keeps going after a failure, prints a summary, and exits non-zero if anything failed. Use `npm run verify -- --skip-build` for a faster loop, but run the full command before opening a pull request.

`verify:device` and `eval` have no checks yet. Recording and model steps fill them in.

When a screen changes on purpose, update its baseline with `npx playwright test --update-snapshots` and attach the before and after images to the pull request. Baselines need Joshua's review.

## Branches and pull requests

- One plan step, one branch, one pull request. Branch: `step-NN-short-name`, for example `step-04-recording`.
- Bug fixes: `fix-short-name`. Commit a test that reproduces the bug first, then the fix. On `fix-*` branches the agent hook asks before a committed test changes.
- Pull request title: `Plan step N`, or a short description for fixes. Fill in the template, including the last lines of `npm run verify`.
- `main` takes changes only through pull requests, squash-merged after CI passes and a reviewer who did not write the code approves.
- Files listed in `.github/CODEOWNERS` control privacy, permissions, dependencies, and agent rules. Changes to them need Joshua's review.

## Agents

`AGENTS.md` holds the rules for coding agents. `CLAUDE.md` only points to it. Skills live in `.claude/skills/`; `.cursor/skills` links to the same folder. Hooks in `.claude/hooks/` are shared by Claude Code (`.claude/settings.json`) and Cursor (`.cursor/hooks.json`).
