#!/usr/bin/python3 -I
# A stand-in `claude` CLI for the end-to-end tests. It answers `--help`, `auth status --json`
# and `-p --output-format stream-json` with lines shaped like real Claude Code 2.1 output
# (a preamble, a Read tool call, then a streamed answer that links the meeting it read).
# Next to this script: `logged-out` (if present) means not signed in; `calls.jsonl` logs
# each run's args, cwd and prompt; `sessions/` keeps sessions for --resume; `claude.pid`
# holds the running answer's pid. A question containing "slow" streams for a minute.
import json, os, re, sys, time, uuid

HERE = os.path.dirname(os.path.abspath(__file__))
HELP = """Usage: claude [options] [command] [prompt]
  -p, --print  --output-format <format>  --verbose  --include-partial-messages
  --tools <tools...>  --disallowedTools <tools...>  --permission-mode <mode> (choices: "acceptEdits", "auto", "bypassPermissions", "manual", "dontAsk", "plan")
  -r, --resume [value]  --safe-mode  --setting-sources <sources>  --strict-mcp-config  --mcp-config <configs...>
  --disable-slash-commands  --no-chrome  --system-prompt <prompt>
"""


def out(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def event(session, e):
    out({"type": "stream_event", "event": e, "session_id": session})


def arg(name):
    a = sys.argv
    return a[a.index(name) + 1] if name in a else None


def logged_out():
    return os.path.exists(os.path.join(HERE, "logged-out"))


def title_of(path):
    m = re.search(r"^title: (.*)$", open(path, encoding="utf-8").read(), re.M)
    return m.group(1).strip() if m else os.path.basename(path)


def pick_meeting(prompt):
    m = re.search(r'<meeting file="([^"]+)">', prompt)
    if m:
        return m.group(1), False
    files = sorted(f for f in os.listdir(".") if f.endswith(".md"))
    launch = [f for f in files if "launch-sync" in f]
    return (launch or files or ["none.md"])[0], True


def stream_text(session, text, delay=0.04):
    event(session, {"type": "message_start", "message": {"role": "assistant", "content": []}})
    event(session, {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    for word in re.findall(r"\S+\s*", text):
        event(session, {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": word}})
        time.sleep(delay)
    out({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}, "session_id": session})


def read_tool(session, path):
    event(session, {"type": "message_start", "message": {"role": "assistant", "content": []}})
    event(session, {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Let me look."}})
    full = os.path.abspath(path)
    out({"type": "assistant", "message": {"role": "assistant", "content": [
        {"type": "text", "text": "Let me look."},
        {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {"file_path": full}}]}, "session_id": session})
    time.sleep(0.3)
    out({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "..."}]}, "session_id": session})


def answer(prompt, session, history):
    question = prompt.rsplit("Question: ", 1)[-1].strip()
    if logged_out():
        out({"type": "assistant", "message": {"content": [{"type": "text", "text": "Not logged in · Please run /login"}]}, "error": "authentication_failed", "session_id": session})
        out({"type": "result", "subtype": "success", "is_error": True, "result": "Not logged in · Please run /login", "session_id": session})
        return
    path, needs_read = pick_meeting(prompt)
    if needs_read:
        read_tool(session, path)
    title = title_of(path) if os.path.exists(path) else path
    if "slow" in question:
        event(session, {"type": "message_start", "message": {"role": "assistant", "content": []}})
        for _ in range(200):
            event(session, {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Still going through it. "}})
            time.sleep(0.3)
    if history:
        text = f"Following up on “{history[0]}”: you own the settings screen, due Friday. See [{title}]({path})."
    else:
        text = f"From [{title}]({path}):\n\n- You committed to **build the settings screen** by Friday.\n- Fast will be the default model.\n"
    stream_text(session, text)
    out({"type": "result", "subtype": "success", "is_error": False, "num_turns": 2, "result": text, "session_id": session})
    history.append(question)


def main():
    if "--help" in sys.argv:
        print(HELP)
        return 0
    if sys.argv[1:3] == ["auth", "status"]:
        print(json.dumps({"loggedIn": not logged_out(), "authMethod": "none" if logged_out() else "claude.ai"}))
        return 1 if logged_out() else 0
    prompt = sys.stdin.read()
    with open(os.path.join(HERE, "calls.jsonl"), "a") as log:
        log.write(json.dumps({"args": sys.argv[1:], "cwd": os.getcwd(), "prompt": prompt}) + "\n")
    with open(os.path.join(HERE, "claude.pid"), "w") as f:
        f.write(str(os.getpid()))
    sessions = os.path.join(HERE, "sessions")
    os.makedirs(sessions, exist_ok=True)
    resume = arg("--resume")
    session = resume or str(uuid.uuid4())
    store = os.path.join(sessions, session + ".json")
    if resume and not os.path.exists(store):
        sys.stderr.write(f"No conversation found with session ID: {resume}\n")
        out({"type": "result", "subtype": "error_during_execution", "is_error": True, "num_turns": 0, "session_id": resume})
        return 1
    history = json.load(open(store)) if resume else []
    out({"type": "system", "subtype": "init", "cwd": os.getcwd(), "session_id": session, "tools": ["Glob", "Grep", "Read"], "permissionMode": arg("--permission-mode")})
    answer(prompt, session, history)
    json.dump(history, open(store, "w"))
    return 0


sys.exit(main())
