# Third-party notices

Anchovy includes or is built with the following software. Add a line when you add a library or a model that ships with the app.

| Component | License                   | Source                                                                                                  |
| --------- | ------------------------- | ------------------------------------------------------------------------------------------------------- |
| Tauri     | MIT or Apache-2.0         | [tauri-apps/tauri](https://github.com/tauri-apps/tauri)                                                 |
| React     | MIT                       | [facebook/react](https://github.com/facebook/react)                                                     |
| Geist     | SIL Open Font License 1.1 | [vercel/geist-font](https://github.com/vercel/geist-font), bundled through `@fontsource-variable/geist` |

The full dependency lists are in `package-lock.json` and `src-tauri/Cargo.lock`. `npm run verify` checks every dependency's license against `privacy/deny.toml`.
