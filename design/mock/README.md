# Interface mock

A static mock of every Anchovy screen, in light and dark (plan step 2a). It uses the app's React,
Tailwind, and Geist with fake data from `src/data.ts`. It does not load Tauri or call Rust.

Once accepted, the screenshots in `screenshots/` are the target the real interface is compared
against from step 2b on.

```sh
# Browse the screens (the list is at the root, each screen at ?screen=<id>&theme=light|dark)
npx vite -c design/mock/vite.config.ts

# Check every screen and rewrite screenshots/
npx playwright test -c design/mock

# Types
npx tsc -p design/mock
```

Screens are listed in `src/catalog.ts`. Proposed color tokens are in `src/tokens.css`. The first
five match `src/styles/index.css`, and step 2b moves the rest into the app.

The meeting prompt is also a macOS notification with the same two buttons. The mock shows only
the banner inside the window.
