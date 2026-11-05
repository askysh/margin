import type { Backends, Settings } from "../api";

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
      <button role="radio" aria-checked={engine === "none"} className={`option ${engine === "none" ? "on" : ""}`} onClick={() => onChange({ ...value, summary_engine: "none" })}>
        <span className="option-title">No summaries</span>
        <span className="option-body">Transcript and notes only. Nothing is sent anywhere.</span>
      </button>
      <button
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
      <button
        role="radio"
        aria-checked={engine === "ollama"}
        disabled={!ollama}
        className={`option ${engine === "ollama" ? "on" : ""}`}
        onClick={() => onChange({ ...value, summary_engine: "ollama", ollama_model: value.ollama_model || backends?.ollama_models[0] || "" })}
      >
        <span className="option-title">Ollama, on this Mac</span>
        <span className="option-body">
          {ollama ? "Fully local. Pick a model you've pulled." : "Ollama isn't running. Start it to use this."}
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
