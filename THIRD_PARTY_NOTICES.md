# Third-party notices

Anchovy includes or is built with the following software. Add a line when you add a library or a model that ships with the app.

| Component       | License                   | Source                                                                                                  |
| --------------- | ------------------------- | ------------------------------------------------------------------------------------------------------- |
| Tauri           | MIT or Apache-2.0         | [tauri-apps/tauri](https://github.com/tauri-apps/tauri)                                                 |
| React           | MIT                       | [facebook/react](https://github.com/facebook/react)                                                     |
| Geist           | SIL Open Font License 1.1 | [vercel/geist-font](https://github.com/vercel/geist-font), bundled through `@fontsource-variable/geist` |
| reqwest         | MIT or Apache-2.0         | [seanmonstar/reqwest](https://github.com/seanmonstar/reqwest), model downloads                          |
| RustCrypto sha2 | MIT or Apache-2.0         | [RustCrypto/hashes](https://github.com/RustCrypto/hashes), model checksums                              |
| objc2           | Zlib, Apache-2.0, or MIT  | [madsmtm/objc2](https://github.com/madsmtm/objc2), Move to Trash and Show in Finder                     |

Models are not bundled. The user downloads them from the list in `src-tauri/resources/models.json`:

| Model                   | License    | Source                                                                                                                                                 |
| ----------------------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Qwen3-ASR 1.7B and 0.6B | Apache-2.0 | [Qwen/Qwen3-ASR-1.7B](https://huggingface.co/Qwen/Qwen3-ASR-1.7B), [Qwen/Qwen3-ASR-0.6B](https://huggingface.co/Qwen/Qwen3-ASR-0.6B), GGUF by ggml-org |
| Qwen3-4B-Instruct-2507  | Apache-2.0 | [Qwen/Qwen3-4B-Instruct-2507](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507), GGUF by unsloth (4-bit) and ggml-org (8-bit)                        |

The full dependency lists are in `package-lock.json` and `src-tauri/Cargo.lock`. `npm run verify` checks every dependency's license against `privacy/deny.toml`.
