# Contributing to Anchovy

Apple Silicon only. macOS 14.4 or later.

## Tools

Versions were checked on Joshua's Mac on 2026-09-27 (macOS 26.6, arm64).

| Tool | Version | How to install |
| --- | --- | --- |
| Rust | 1.98.1, with rustfmt and clippy | [rustup](https://rustup.rs), stable. `rust-toolchain.toml` selects 1.98.1 when you enter this repository. |
| Node.js | 22.23.3 | Homebrew `node@22`, or nvm / fnm. `.node-version` records the version. |
| npm | 10.9.9 | Comes with that Node.js. |
| CMake | 4.4.3 | `brew install cmake` |
| Xcode | 27.0 | Mac App Store, plus the command line tools |

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

## Not written yet

How to run the app, how to test, and how to name branches arrive with plan step 0. Until then this repository is the README, the icon, and these version pins.
