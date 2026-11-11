import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, errorText } from "../api";
import { copyText } from "../util";
import { Logo } from "./Logo";
import { MicIcon, PermissionRow, SpeakerIcon } from "./PermissionRow";
import { Card, Row, SectionHead, ToggleRow } from "./SettingsRows";
import { restart, useUpdateStatus } from "./Updates";
import type { SettingsView } from "./Settings";

export function VoicesSection({ voices, setVoices, notify }: SettingsView) {
  const forget = async (slug: string, name: string) => {
    const sure = await ask(`Margin stops labelling ${name} by voice in new meetings. Meetings that already name ${name} stay as they are.`, {
      title: `Forget ${name}'s voice?`,
      kind: "warning",
      okLabel: "Forget",
      cancelLabel: "Cancel",
    });
    if (!sure) return;
    try {
      await api.forgetVoice(slug);
      notify(`Margin forgot ${name}'s voice.`);
    } catch (e) {
      notify(`Couldn't forget ${name}'s voice: ${errorText(e)}`, "error");
    }
    setVoices(await api.listVoices().catch(() => voices));
  };
  return (
    <>
      <SectionHead title="Voices" about="People Margin recognises by voice and labels by name in new meetings." />
      {voices.length === 0 ? (
        <Card>
          <p className="set-empty">
            <strong>No voices yet.</strong> Rename a speaker in a meeting and keep “Recognise this voice” checked.
            Margin then labels that person in later meetings.
          </p>
        </Card>
      ) : (
        <Card title="Voices Margin recognises">
          <ul className="set-list" aria-label="Voices Margin recognises">
            {voices.map((v) => (
              <li key={v.slug}>
                <Row title={v.name} help={`${v.samples} sample${v.samples === 1 ? "" : "s"}`}>
                  <button className="btn btn-quiet btn-danger-quiet" aria-label={`Forget ${v.name}`} onClick={() => forget(v.slug, v.name)}>Forget</button>
                </Row>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </>
  );
}

export function PermissionsSection({ perms, refreshPerms }: SettingsView) {
  return (
    <>
      <SectionHead title="Permissions" about="What macOS lets Margin hear. Margin checks again each time you come back to this window." />
      <Card>
        <PermissionRow
          title="Microphone"
          body="Needed for every recording."
          icon={<MicIcon />}
          state={perms?.microphone}
          onRequest={async () => { await api.requestPermission("microphone"); await refreshPerms(); }}
          onSettings={() => api.openPrivacySettings("microphone")}
        />
        <PermissionRow
          title="System audio"
          body="Needed to hear the other people on calls."
          icon={<SpeakerIcon />}
          state={perms?.system_audio}
          onRequest={async () => { await api.requestPermission("system-audio"); await refreshPerms(); }}
          onSettings={() => api.openPrivacySettings("system-audio")}
        />
      </Card>
    </>
  );
}

export function IntegrationsSection({ mcp, notify }: SettingsView) {
  const copy = async () => notify((await copyText(mcp)) ? "Copied." : "Couldn't copy. Select the text and press ⌘C.");
  return (
    <>
      <SectionHead title="Integrations" about="Use your meetings from other apps." />
      <Card title="Use your meetings in Claude">
        <Row title="Minutes MCP server" help="Add this to your Claude Desktop or Claude Code MCP config to search and read your meetings from Claude. It uses the open-source Minutes MCP server.">
          <button className="btn btn-quiet" onClick={copy}>Copy</button>
        </Row>
        <div className="set-pad">
          <pre className="code">{mcp}</pre>
        </div>
      </Card>
    </>
  );
}

export function UpdatesSection({ prefs, changePrefs, notify }: SettingsView) {
  const [status, setStatus] = useUpdateStatus();
  const [asked, setAsked] = useState(false);
  const check = async () => {
    setAsked(true);
    try {
      setStatus(await api.checkUpdates());
    } catch (e) {
      notify(errorText(e), "error");
    }
  };
  const line = !status
    ? ""
    : !status.supported
      ? "Updates work only in the installed app."
      : status.checking
        ? "Checking…"
        : status.ready
          ? `Margin ${status.ready} is downloaded. It installs when you restart or quit Margin.`
          : status.error || (asked || status.checked_secs_ago !== null ? "Margin is up to date." : "");
  return (
    <>
      <SectionHead title="Updates" about="New versions of Margin, from GitHub." />
      <Card>
        <Row title={<>Margin <span className="mono">{status?.current ?? ""}</span></>} help={<span role="status">{line}</span>}>
          {status?.ready ? (
            <button className="btn btn-primary" onClick={() => restart(notify)}>Restart to update</button>
          ) : (
            <button className="btn btn-quiet" onClick={check} disabled={!status?.supported || status.checking}>Check now</button>
          )}
        </Row>
        <ToggleRow
          title="Update automatically"
          help="Check for updates and download them in the background. They install when you restart or quit Margin."
          checked={prefs.auto_update}
          onChange={(on) => changePrefs({ ...prefs, auto_update: on })}
        />
      </Card>
    </>
  );
}

export function AboutSection({ boot }: SettingsView) {
  return (
    <>
      <header className="set-about">
        <Logo size={56} />
        <div>
          <h1 id="set-title" tabIndex={-1}>Margin</h1>
          <p>A meeting notepad for macOS. Notes in your words, transcribed on your Mac.</p>
        </div>
      </header>
      <Card note="Margin and minutes-core are MIT licensed.">
        <Row title="Version"><span className="mono set-value">{boot.version}</span></Row>
        <Row title="Built on minutes-core" help="Local transcription, speaker separation, voice recognition and the Markdown files.">
          <span className="mono set-value">{boot.upstream_tag}</span>
        </Row>
      </Card>
    </>
  );
}

export function ResetSection({ openReset }: SettingsView) {
  return (
    <>
      <SectionHead title="Start over" about="Reset Margin to a fresh install." danger />
      <Card danger>
        <Row title="Reset Margin" help="Clears your settings and runs setup again. You choose what happens to your meetings, speech models and voices.">
          <button className="btn btn-danger" onClick={(e) => openReset(e.currentTarget)}>Reset Margin…</button>
        </Row>
      </Card>
    </>
  );
}
