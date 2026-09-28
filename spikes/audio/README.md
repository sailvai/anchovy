# Audio spike (plan step 4a)

A throwaway program, not the recorder. It puts the selected microphone and a
global system-audio tap into one private aggregate device, records 10 seconds,
and writes every channel to one 32-bit float WAV file (microphone channels
first, then the tap) with a JSON report of each channel's level and the level
of a test tone.

It is a separate Cargo project. The app build and `npm run verify` do not
compile it.

```sh
cargo test
cargo run -- list                      # input devices, * marks the default
cargo run -- tone /tmp/tone.wav        # 12 s, 1 kHz test tone
afplay /tmp/tone.wav & cargo run -- record [--device <name part>] [--out <dir>]
```

To test permission prompts and the App Sandbox, wrap it in an ad-hoc signed app
so macOS treats it as its own app:

```sh
./bundle.sh sandboxed /tmp/spike       # or: unsandboxed
open -W -n --stdout /tmp/spike/out.txt --stderr /tmp/spike/err.txt \
  "/tmp/spike/Audio Spike Sandboxed.app" --args record
```

The sandboxed build writes to its container:
`~/Library/Containers/com.sailvai.anchovy.audiospike.sandboxed/Data/tmp/`.
