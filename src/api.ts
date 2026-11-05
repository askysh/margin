import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Kind = "call" | "in-person";

export interface Settings {
  output_dir: string;
  model: string;
  summary_engine: "none" | "claude" | "ollama";
  claude_path: string | null;
  ollama_model: string;
  user_name: string;
}

export interface LiveLine {
  line: number;
  offset_ms: number;
  text: string;
  speaker: "you" | "them" | null;
}

export interface RecorderView {
  recording: boolean;
  stopping: boolean;
  kind: Kind | null;
  title: string | null;
  started_at: string | null;
  elapsed_secs: number;
  level: number;
  lines: LiveLine[];
  notes: string;
}

export interface Boot {
  onboarded: boolean;
  settings: Settings;
  recorder: RecorderView;
  orphaned_draft: string | null;
  upstream_tag: string;
  version: string;
}

export interface MeetingSummary {
  path: string;
  title: string;
  date: string | null;
  duration: string | null;
  kind: string;
  status: string | null;
  snippet: string;
}

export interface Turn {
  speaker: string | null;
  ts: string;
  text: string;
  source: "MICROPHONE" | "SYSTEM" | null;
}

export interface Speaker {
  label: string;
  name: string | null;
  turns: number;
}

export interface MeetingDetail extends MeetingSummary {
  attendees: string[];
  sections: { heading: string; body: string }[];
  notes: string;
  transcript: Turn[];
  speakers: Speaker[];
  in_obsidian_vault: boolean;
  has_audio: boolean;
}

export type JobState =
  | "queued" | "transcribing" | "transcript-only" | "diarizing" | "summarizing"
  | "saving" | "needs-review" | "complete" | "failed";

export interface Job {
  id: string;
  title: string | null;
  state: JobState;
  stage: string | null;
  output_path: string | null;
  created_at: string;
  error: string | null;
  draft: string | null;
}

export interface ModelFile {
  id: string;
  label: string;
  url: string;
  approx_bytes: number;
  installed: boolean;
}

export interface ModelProgress {
  id: string;
  downloaded: number;
  total: number;
  finished: boolean;
  all_done: boolean;
  error: string | null;
}

export type PermState = "granted" | "denied" | "not-determined" | "unknown";
export interface Permissions {
  microphone: PermState;
  system_audio: PermState;
}

export interface Backends {
  claude_path: string | null;
  ollama_running: boolean;
  ollama_models: string[];
}

export interface ResetPreview {
  output_dir: string;
  meeting_count: number;
  models_bytes: number;
  voice_count: number;
  upstream_app_installed: boolean;
}

export interface Voice {
  slug: string;
  name: string;
  samples: number;
}

export const api = {
  boot: () => invoke<Boot>("boot"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  folderChoices: () =>
    invoke<{ default_dir: string; obsidian_vaults: { name: string; path: string }[] }>("folder_choices"),
  completeOnboarding: () => invoke<void>("complete_onboarding"),
  modelsStatus: () => invoke<ModelFile[]>("models_status"),
  downloadModels: () => invoke<void>("download_models"),
  permissions: () => invoke<Permissions>("permissions_state"),
  requestPermission: (which: "microphone" | "system-audio") => invoke<PermState>("request_permission", { which }),
  openPrivacySettings: (pane: "microphone" | "system-audio") => invoke<void>("open_privacy_settings", { pane }),
  summaryBackends: () => invoke<Backends>("summary_backends"),
  startRecording: (kind: Kind, title?: string) => invoke<RecorderView>("start_recording", { kind, title: title ?? null }),
  stopRecording: (notes: string | null, title: string | null) =>
    invoke<{ job_id: string; title: string | null }>("stop_recording", { notes, title }),
  recorderView: () => invoke<RecorderView>("recorder_view"),
  setDraftNotes: (text: string) => invoke<void>("set_draft_notes", { text }),
  setRecordingTitle: (title: string) => invoke<void>("set_recording_title", { title }),
  discardOrphanedDraft: () => invoke<void>("discard_orphaned_draft"),
  listJobs: () => invoke<Job[]>("list_jobs"),
  retryJob: (id: string) => invoke<void>("retry_job", { id }),
  listMeetings: () => invoke<MeetingSummary[]>("list_meetings"),
  readMeeting: (path: string) => invoke<MeetingDetail>("read_meeting", { path }),
  saveNotes: (path: string, notes: string) => invoke<void>("save_notes", { path, notes }),
  setMeetingTitle: (path: string, title: string) => invoke<void>("set_meeting_title", { path, title }),
  search: (query: string) => invoke<{ path: string; excerpt: string }[]>("search_meetings", { query }),
  trashMeeting: (path: string) => invoke<void>("trash_meeting", { path }),
  reveal: (path: string) => invoke<void>("reveal_in_finder", { path }),
  openInObsidian: (path: string) => invoke<void>("open_in_obsidian", { path }),
  renameSpeaker: (path: string, label: string, name: string, rememberVoice: boolean) =>
    invoke<{ voice_saved: boolean; note: string | null }>("rename_speaker", { path, label, name, rememberVoice }),
  summarize: (path: string) => invoke<void>("summarize_meeting", { path }),
  listVoices: () => invoke<Voice[]>("list_voices"),
  forgetVoice: (slug: string) => invoke<void>("forget_voice", { slug }),
  resetPreview: () => invoke<ResetPreview>("reset_preview"),
  reset: (options: { delete_meetings: boolean; delete_models: boolean; forget_voices: boolean }) =>
    invoke<{ meetings_trashed: number; models_deleted: boolean; voices_forgotten: number }>("reset_app", { options }),
  mcpSnippet: () => invoke<string>("mcp_snippet"),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
