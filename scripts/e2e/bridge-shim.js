// Injected before the app loads in the end-to-end tests. Stands in for Tauri's IPC: every
// invoke goes to the real backend (`margin --dev serve`) over HTTP, and backend events are
// long-polled and delivered to listeners the way @tauri-apps/api expects.
// Exposes window.__E2E__ for tests: pickFolder (dialog result), calls (log), errors, emit.
(() => {
  const BRIDGE = window.__E2E_BRIDGE__ || "http://127.0.0.1:4545";
  const callbacks = new Map();
  const listeners = new Map(); // event -> Set(handlerId)
  let nextId = 1;
  const e2e = (window.__E2E__ = { pickFolder: null, calls: [], errors: [] });

  async function call(cmd, args) {
    e2e.calls.push(cmd);
    const r = await fetch(`${BRIDGE}/invoke`, { method: "POST", body: JSON.stringify({ cmd, args: args || {} }) });
    const body = await r.json();
    if (!body.ok) {
      e2e.errors.push(`${cmd}: ${body.error}`);
      throw body.error;
    }
    return body.value;
  }

  const deliver = (event, seq, payload) => {
    for (const id of listeners.get(event) || []) callbacks.get(id)?.({ event, id: seq, payload });
  };
  // Perf tests: deliver a synthetic backend event, as if the bridge had sent it.
  e2e.emit = (event, payload) => deliver(event, 0, payload);

  async function poll(since) {
    try {
      const r = await fetch(`${BRIDGE}/events?since=${since}`);
      const items = await r.json();
      for (const { seq, event, payload } of items) {
        since = Math.max(since, seq);
        deliver(event, seq, payload);
      }
    } catch (e) {
      await new Promise((res) => setTimeout(res, 500));
    }
    poll(since);
  }
  // Start from "now": skip events emitted before this page loaded.
  fetch(`${BRIDGE}/latest`).then((r) => r.json()).then((seq) => poll(seq), () => poll(0));

  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener(event, id) {
      listeners.get(event)?.delete(id);
    },
  };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback(cb) {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    convertFileSrc: (p) => p,
    async invoke(cmd, args) {
      if (cmd === "plugin:event|listen") {
        if (!listeners.has(args.event)) listeners.set(args.event, new Set());
        listeners.get(args.event).add(args.handler);
        return args.handler;
      }
      if (cmd === "plugin:event|unlisten") {
        listeners.get(args.event)?.delete(args.eventId);
        return null;
      }
      if (cmd === "plugin:dialog|open") return e2e.pickFolder;
      if (cmd === "plugin:dialog|ask" || cmd === "plugin:dialog|confirm") return e2e.answer ?? true;
      if (cmd.startsWith("plugin:")) return null;
      return call(cmd, args);
    },
  };
})();
