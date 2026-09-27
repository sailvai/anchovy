---
name: anchovy-ui
description: Anchovy interface rules and the screenshot loop. Use when building or changing anything under src/, making the design mock, writing interface copy, or reviewing or updating screenshots.
---

# Anchovy interface

## Look

- Quiet and gray. Grays carry the chrome; color only signals state: recording, failed, ready, selected.
- Geist, bundled with the app (`@fontsource-variable/geist`). Never load fonts or anything else from a CDN.
- Separate areas with thin lines, not heavy shadows.
- Light and dark mode both work. Dark mode follows macOS. Colors are tokens in `src/styles/index.css`; do not hard-code colors in components.

## Structure

- One window. A list of recordings on the left, newest first; the current item on the right.
- Models and Settings replace the right side. They never open a new window.
- One primary button per flow. Other actions are secondary or live in a menu.
- Prompts that ask for a decision do not block the window unless the decision is needed to continue.
- Destructive actions move files to the Trash. Overwriting a user's note asks first.
- Show the real state honestly: which inputs are recording, what failed and why, what a denied permission means.

## Copy

- English only. `npm run verify` fails on Chinese, Japanese, or Korean characters in `src/` outside tests.
- Short and literal. Say what happens. Product names (Sailvai, Anchovy) are never translated.

## Screenshot loop

1. Screens render in a browser with fake Rust commands from `tests/ui/fake-ipc.ts`. Add a fake for every new command.
2. Each screen gets a light and a dark screenshot test in `tests/ui/`.
3. Implement, run `npx playwright test`, look at the screenshots, compare with the accepted mock in `design/mock/`, adjust. Two or three rounds is normal.
4. Update baselines only on purpose: `npx playwright test --update-snapshots`. Baselines are protected; attach before and after images to the pull request.
