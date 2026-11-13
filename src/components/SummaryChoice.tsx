import type { Backends, Settings } from "../api";
import { DEVICE } from "../platform";

interface Props {
  backends: Backends | null;
  value: Partial<Settings>;
  onChange: (v: Partial<Settings>) => void;
}

export function SummaryChoice({ backends, value, onChange }: Props) {
  const engine = value.summary_engine ?? "none";
  const claude = backends?.claude_path;
  const ollama = backends?.ollama_running;
  return (
    <div className="options" role="radiogroup">
      <button type="button" role="radio" aria-checked={engine === "none"} className={`option ${engine === "none" ? "on" : ""}`} onClick={() => onChange({ ...value, summary_engine: "none" })}>
        <span className="option-title">No summaries</span>
        <span className="option-body">Transcript and notes only. Nothing is sent anywhere.</span>
      </button>
      <button type="button"
        role="radio"
        aria-checked={engine === "claude"}
        disabled={!claude}
        className={`option ${engine === "claude" ? "on" : ""}`}
        onClick={() => onChange({ ...value, summary_engine: "claude", claude_path: claude ?? null })}
      >
        <span className="option-title">Claude, through your Claude Code CLI</span>
        <span className="option-body">
          {claude ? "Uses your own Claude account. The transcript is sent to Anthropic to summarize." : "Not found. Install Claude Code to use this."}
        </span>
        {claude && <span className="option-path path">{claude}</span>}
      </button>
      <button type="button"
        role="radio"
        aria-checked={engine === "ollama"}
        disabled={!ollama || !backends?.ollama_models.length}
        className={`option ${engine === "ollama" ? "on" : ""}`}
        onClick={() => {
          const installed = backends?.ollama_models ?? [];
          const model = value.ollama_model && installed.includes(value.ollama_model) ? value.ollama_model : installed[0] ?? "";
          onChange({ ...value, summary_engine: "ollama", ollama_model: model });
        }}
      >
        <span className="option-title">Ollama, on this {DEVICE}</span>
        <span className="option-body">
          {!ollama
            ? "Ollama isn't running. Start it to use this."
            : backends?.ollama_models.length
              ? `Fully local. Nothing leaves this ${DEVICE}.`
              : "Ollama has no models yet. Run `ollama pull llama3.2` first."}
        </span>
      </button>
      {ollama && engine === "ollama" && (
        <label className="option-sub">
          Model
          <select
            className="field field-inline"
            value={value.ollama_model}
            onChange={(e) => onChange({ ...value, ollama_model: e.target.value })}
          >
            {(backends?.ollama_models ?? []).map((m) => <option key={m} value={m}>{m}</option>)}
          </select>
        </label>
      )}
    </div>
  );
}
