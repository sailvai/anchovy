# Sailvai Anchovy

Free, open-source meeting notes for Apple Silicon Macs. Record your microphone and computer audio, transcribe and summarize on the machine, and save a Markdown note next to the audio. No account. No upload.

![Anchovy](brand/anchovy.png)

The app is not available for download yet. Right now the repository builds an empty Anchovy window; features arrive step by step.

## Run it on your Mac

You need an Apple Silicon Mac with macOS 14.4 or later, plus the tools listed in [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
npm install
npm run tauri dev
```

To run every check, as CI does:

```sh
npm run verify
```

## Known limits

- In meetings that mix Chinese and English, the transcript sometimes writes Chinese speech as English, and the notes may translate decisions and action items into the other language.
- Decisions and action items can include things that were only mentioned, such as a status update, a problem someone described, or a question, and they can miss some. Check them against the transcript, which is in the same note.
- Writing notes for a long meeting needs several gigabytes of free memory. If other apps are using too much, for example a browser or a video call, Anchovy says there is not enough memory and does not write the note. Close some apps, then choose Retry. The recording is kept. The transcript is not, so Retry transcribes the meeting again.

Sailvai is the company. Anchovy is its first product.
