# Sailvai Anchovy

Free, open-source meeting notes for Apple Silicon Macs. Record your microphone and computer audio, transcribe and summarize on the Mac, and save a Markdown note next to the audio. No account. Nothing is uploaded.

![Anchovy](brand/anchovy.png)

## What it does

- Press Record. Anchovy records your microphone and the sound your Mac is playing into one audio file.
- When the recording stops, Anchovy transcribes it and writes a summary, decisions, and action items. Both steps run on your Mac.
- The note is a Markdown file saved in the same folder as the audio, inside the notes folder you chose. Any tool that reads Markdown can open it, and the notes folder can be an Obsidian vault.
- When a Zoom, Google Meet, or Microsoft Teams meeting starts, Anchovy asks whether to record it. It records only if you choose Record.

## What it runs on

An Apple Silicon Mac with macOS 14.4 or later. Anchovy does not run on Intel Macs, iPhone, or Windows.

## Build it

There is no download yet. You build the app on your own Mac.

Install the pinned tools listed in [CONTRIBUTING.md](CONTRIBUTING.md), then run:

```sh
npm install
npm run tauri build
```

The build writes an unsigned app to:

```text
src-tauri/target/release/bundle/macos/Anchovy.app
```

Drag it to your Applications folder, or open it where it is.

## Open the app the first time

The app you built is unsigned, so macOS can block the first open with a message that the developer cannot be verified.

1. Right-click Anchovy and choose Open.
2. In the message that appears, choose Open again.

If macOS still blocks it, open System Settings > Privacy & Security, find the message about Anchovy, and allow it.

On first open, Anchovy asks for a notes folder, then for the two audio permissions, then offers to download the models. You can choose Later and download them from Models when you are ready; recordings are saved either way.

## Permissions

- **Microphone.** This is your side of the meeting.
- **Computer audio.** This is the other side of the meeting, and any other sound your Mac plays.
- **Notes folder.** This is where recordings and notes are saved.

If you do not allow computer audio, recording still works, but the other side of an online meeting is not recorded. Anchovy shows which sources it is recording, and the note lists them.

## Privacy

- Recordings, transcripts, and notes stay on this Mac.
- The network is used to download the models.
- There is no account and no analytics.
- After the models are downloaded, recording and notes work with the network off.

## Known limits

- In meetings that mix Chinese and English, the transcript sometimes writes Chinese speech as English, and the notes may translate decisions and action items into the other language.
- Decisions and action items can include things that were only mentioned, such as a status update, a problem someone described, or a question, and they can miss some. Check them against the transcript, which is in the same note.
- Writing notes for a long meeting needs several gigabytes of free memory. If other apps are using too much, for example a browser or a video call, Anchovy says there is not enough memory and does not write the note. Close some apps, then choose Retry. The recording is kept. The transcript is not, so Retry transcribes the meeting again.
- A Mac with 8 GB of memory has not been tested. Anchovy is designed so that the speech model and the summary model are not loaded at the same time, but that has not been tried on a real 8 GB machine.

## Work on it

[CONTRIBUTING.md](CONTRIBUTING.md) covers running the app during development, the checks, and how pull requests work.

Sailvai is the company. Anchovy is its first product. The code is under the [MIT license](LICENSE).
