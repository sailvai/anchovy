# ASR spike (plan step 6a)

A throwaway program, not the engine. It runs Qwen3-ASR from the shipped model
list through `llama-cpp-2` with `mtmd` and Metal on one clip, and prints the
transcript, the timings, and an estimate for one hour of audio.

It is a separate Cargo project. The app build and `npm run verify` do not
compile it.

`fetch` downloads the model through the app's own model store into
`~/Library/Application Support/Anchovy/Models/`. `run` hashes every model file
again and stops if a `sha256` does not match the shipped list.

The clips are read by macOS's built-in voices from the scripts in `clips/`.
The scripts are committed; the audio is not.

```sh
cargo test
cargo build --release
./target/release/asr-spike fetch                      # qwen3-asr-1.7b
./make-clips.sh /tmp/asr-clips
/usr/bin/time -l ./target/release/asr-spike run /tmp/asr-clips/zh.wav --script clips/zh.txt
/usr/bin/time -l ./target/release/asr-spike run /tmp/asr-clips/en.wav --script clips/en.txt --words
```

`--model qwen3-asr-0.6b` runs the smaller model after `fetch qwen3-asr-0.6b`.
Peak memory is the "peak memory footprint" line from `/usr/bin/time -l`, which
counts Metal buffers.
