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
  // Filled by the backend. Leave a field out of a save to keep its stored value.
  transcription_language?: string;
  summary_language?: string;
  input_device?: string;
  speaker_separation?: boolean;
  silence_stop_minutes?: number;
  keep_audio_days?: number;
  call_detection?: boolean;
  vocabulary?: string[];
  summary_instructions?: string;
  open_at_login?: boolean;
}

export interface SettingsOptions {
  languages: { code: string; name: string }[];
  input_devices: string[];
  silence_stop_minutes: number[];
  keep_audio_days: number[];
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

/** Margin's own preferences (prefs.json), not minutes-core's config. */
export interface AppPrefs {
  sound_cues: boolean;
  record_shortcut: boolean;
  auto_update: boolean;
}

/** The updater's state. `ready` is a newer version that is downloaded. */
export interface UpdateStatus {
  supported: boolean;
  current: string;
  ready: string | null;
  notes: string | null;
  checking: boolean;
  checked_secs_ago: number | null;
  error: string | null;
}

export interface Boot {
  onboarded: boolean;
  settings: Settings;
  prefs: AppPrefs;
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
  people: string[];
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
  action_items: ActionItem[];
  /** Margin is writing this meeting's summary now. */
  summarizing: boolean;
}

export interface ActionItem {
  task: string;
  assignee: string | null;
  due: string | null;
  done: boolean;
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

export interface ChatSource {
  path: string;
  title: string;
}

export interface ChatMessage {
  role: "user" | "assistant";
  text: string;
  sources: ChatSource[];
  error: string | null;
  stopped: boolean;
  at: string;
}

/** One Ask conversation: about one meeting, or across all of them (meeting = null). */
export interface ChatThread {
  key: string;
  meeting: string | null;
  session_id: string | null;
  messages: ChatMessage[];
  running: boolean;
  partial: string;
  activity: string | null;
  elapsed_secs: number;
}

export interface ChatReadiness {
  ready: boolean;
  problem: "not-installed" | "not-signed-in" | "too-old" | null;
  message: string | null;
}

export const api = {
  boot: () => invoke<Boot>("boot"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  settingsOptions: () => invoke<SettingsOptions>("settings_options"),
  saveAppPrefs: (prefs: AppPrefs) => invoke<AppPrefs>("save_app_prefs", { prefs }),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  checkUpdates: () => invoke<UpdateStatus>("check_updates"),
  installUpdate: () => invoke<void>("install_update"),
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
  /** The live transcript lines kept with a job, shown as a draft until the meeting opens. */
  liveDraft: (jobId: string) => invoke<LiveLine[]>("live_draft", { jobId }),
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
  setActionItem: (path: string, index: number, done: boolean) => invoke<void>("set_action_item", { path, index, done }),
  /** The recording's file path, readable through the asset protocol (convertFileSrc). */
  meetingAudio: (path: string) => invoke<string>("meeting_audio", { path }),
  listVoices: () => invoke<Voice[]>("list_voices"),
  forgetVoice: (slug: string) => invoke<void>("forget_voice", { slug }),
  resetPreview: () => invoke<ResetPreview>("reset_preview"),
  reset: (options: { delete_meetings: boolean; delete_models: boolean; forget_voices: boolean }) =>
    invoke<{ meetings_trashed: number; models_deleted: boolean; voices_forgotten: number }>("reset_app", { options }),
  mcpSnippet: () => invoke<string>("mcp_snippet"),
  chatStatus: () => invoke<ChatReadiness>("chat_status"),
  chatThread: (meeting: string | null) => invoke<ChatThread>("chat_thread", { meeting }),
  chatSend: (meeting: string | null, text: string) => invoke<ChatThread>("chat_send", { meeting, text }),
  chatStop: (meeting: string | null) => invoke<void>("chat_stop", { meeting }),
  chatClear: (meeting: string | null) => invoke<void>("chat_clear", { meeting }),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
