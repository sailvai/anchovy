import type { ScreenId } from "../catalog";
import { liveRecording, recordings } from "../data";
import { NotePane } from "./Note";
import { OnboardingAudio, OnboardingFolder, OnboardingModels } from "./Onboarding";
import { ModelsPane, SettingsPane } from "./Places";
import { Home, RecordingPane } from "./Record";
import { MeetingBanner, Window } from "./Window";

const folderFor = {
  saved: "2026-09-25-1000",
  "needs-models": "2026-09-26-1130",
  working: "2026-09-26-1410",
  failed: "2026-09-26-0905",
  ready: "2026-09-25-1645",
} as const;

export function Screen({ id }: { id: ScreenId }) {
  switch (id) {
    case "onboarding-folder":
      return <OnboardingFolder />;
    case "onboarding-audio":
      return <OnboardingAudio />;
    case "onboarding-audio-denied":
      return <OnboardingAudio denied />;
    case "onboarding-models":
      return <OnboardingModels />;
    case "onboarding-models-downloading":
      return <OnboardingModels downloading />;
    case "library-empty":
      return (
        <Window recordings={[]}>
          <Home />
        </Window>
      );
    case "library":
      return (
        <Window>
          <Home />
        </Window>
      );
    case "library-mic-only":
      return (
        <Window>
          <Home computerAudio="not-allowed" />
        </Window>
      );
    case "recording":
      return (
        <Window
          recordings={[liveRecording, ...recordings]}
          selected={liveRecording.folder}
          recording
        >
          <RecordingPane />
        </Window>
      );
    case "note-saved":
    case "note-needs-models":
    case "note-working":
    case "note-failed":
    case "note-ready": {
      const state = id.slice("note-".length) as keyof typeof folderFor;
      return (
        <Window selected={folderFor[state]}>
          <NotePane state={state} />
        </Window>
      );
    }
    case "note-menu":
      return (
        <Window selected={folderFor.ready}>
          <NotePane state="ready" menu />
        </Window>
      );
    case "note-regenerate":
      return (
        <Window selected={folderFor.ready}>
          <NotePane state="ready" confirm />
        </Window>
      );
    case "models":
      return (
        <Window place="models">
          <ModelsPane />
        </Window>
      );
    case "settings":
      return (
        <Window place="settings">
          <SettingsPane />
        </Window>
      );
    case "meeting-banner":
      return (
        <Window banner={<MeetingBanner app="Zoom" />} selected={folderFor.ready}>
          <NotePane state="ready" />
        </Window>
      );
  }
}
