import type { ReactNode } from "react";
import { notesFolder, summaryModels, transcriptionModels } from "../data";
import { CheckIcon, FolderIcon, MicIcon, ModelsIcon, SpeakerIcon } from "../icons";
import { Button, Progress } from "../ui";

// First launch: three steps, one screen each, one primary button per screen.
function Step({
  step,
  title,
  body,
  children,
  actions,
}: {
  step: number;
  title: string;
  body: ReactNode;
  children: ReactNode;
  actions: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col bg-surface">
      <div className="flex flex-1 items-center justify-center px-8">
        <div className="w-[460px]">
          <p className="text-[12px] font-medium text-muted">Anchovy setup · Step {step} of 3</p>
          <h1 className="mt-2 text-[22px] font-semibold tracking-[-0.01em]">{title}</h1>
          <p className="mt-2 text-[13px] text-muted">{body}</p>
          <div className="mt-6">{children}</div>
        </div>
      </div>
      <div className="flex items-center justify-between border-t border-line px-6 py-4">
        <StepDots current={step} />
        <div className="flex gap-2">{actions}</div>
      </div>
    </div>
  );
}

function StepDots({ current }: { current: number }) {
  return (
    <div className="flex gap-1.5" aria-label={`Step ${current} of 3`}>
      {[1, 2, 3].map((step) => (
        <span
          key={step}
          className={`h-1 w-6 rounded-full ${step <= current ? "bg-text" : "bg-line-strong"}`}
        />
      ))}
    </div>
  );
}

function Row({
  icon,
  title,
  body,
  aside,
  children,
}: {
  icon: ReactNode;
  title: string;
  body: ReactNode;
  aside?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="px-4 py-3">
      <div className="flex items-start gap-3">
        <span className="mt-0.5 text-muted">{icon}</span>
        <div className="min-w-0 flex-1">
          <p className="font-medium">{title}</p>
          <p className="text-[12px] text-muted">{body}</p>
        </div>
        {aside}
      </div>
      {children}
    </div>
  );
}

function Box({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-line rounded-lg border border-line">{children}</div>;
}

function Allowed() {
  return (
    <span className="inline-flex h-7 items-center gap-1 text-[12px] font-medium text-ready">
      <CheckIcon className="size-3.5" />
      Allowed
    </span>
  );
}

export function OnboardingFolder() {
  return (
    <Step
      step={1}
      title="Choose a notes folder"
      body="Anchovy saves each recording and its note here as plain files. Any app that reads Markdown can open them, including an Obsidian vault."
      actions={<Button variant="primary">Continue</Button>}
    >
      <Box>
        <Row
          icon={<FolderIcon />}
          title={notesFolder}
          body="New folder. Anchovy creates it when you continue."
          aside={<Button size="sm">Choose Folder…</Button>}
        />
      </Box>
      <p className="mt-3 text-[12px] text-muted">
        To use an existing Obsidian vault, choose the vault folder. You can change this later in
        Settings.
      </p>
    </Step>
  );
}

export function OnboardingAudio({ denied = false }: { denied?: boolean }) {
  return (
    <Step
      step={2}
      title="Allow audio"
      body="Anchovy records only when you press Record or accept a meeting prompt. Audio stays on this Mac."
      actions={
        <>
          <Button variant="ghost">Back</Button>
          <Button variant="primary">Continue</Button>
        </>
      }
    >
      <Box>
        <Row
          icon={<MicIcon />}
          title="Microphone"
          body="Records your voice. Needed to record."
          aside={<Allowed />}
        />
        <Row
          icon={<SpeakerIcon />}
          title="Computer audio"
          body="Records the other side of calls."
          aside={
            denied ? (
              <span className="inline-flex h-7 items-center text-[12px] font-medium text-attention">
                Not allowed
              </span>
            ) : (
              <Button size="sm">Allow</Button>
            )
          }
        >
          {denied && (
            <div className="mt-3 ml-7 rounded-md bg-attention-soft px-3 py-2.5 text-[12px]">
              <p>
                You can continue. Anchovy will record only your microphone, so the other side of
                online meetings will not be recorded.
              </p>
              <Button size="sm" className="mt-2">
                Open System Settings
              </Button>
            </div>
          )}
        </Row>
      </Box>
    </Step>
  );
}

export function OnboardingModels({ downloading = false }: { downloading?: boolean }) {
  const models = [
    { ...transcriptionModels[0], role: "Transcription", state: downloading ? "2.1 of 3.4 GB" : "" },
    { ...summaryModels[0], role: "Summary", state: downloading ? "Waiting" : "" },
  ];
  return (
    <Step
      step={3}
      title="Download the models"
      body="Anchovy writes notes with two models that run on this Mac. You can record now and download later in Models."
      actions={
        downloading ? (
          <Button variant="primary">Continue</Button>
        ) : (
          <>
            <Button variant="ghost">Later</Button>
            <Button variant="primary">Download 5.7 GB</Button>
          </>
        )
      }
    >
      <Box>
        {models.map((model) => (
          <Row
            key={model.name}
            icon={<ModelsIcon />}
            title={model.name}
            body={`${model.role} · ${model.license}`}
            aside={
              <span className="flex h-5 items-center text-[12px] text-muted tabular-nums">
                {model.state || model.size}
              </span>
            }
          />
        ))}
      </Box>
      {downloading ? (
        <div className="mt-4">
          <div className="flex justify-between text-[12px] tabular-nums">
            <span>Downloading 2.1 of 5.7 GB</span>
            <span className="text-muted">About 6 minutes left</span>
          </div>
          <Progress value={37} className="mt-2" />
          <p className="mt-3 text-[12px] text-muted">
            The download continues in the background. Recordings you make now get their notes when
            it finishes.
          </p>
        </div>
      ) : (
        <p className="mt-3 text-[12px] text-muted tabular-nums">
          Total 5.7 GB. Anchovy checks each file after it downloads.
        </p>
      )}
    </Step>
  );
}
