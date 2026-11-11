# A stand-in Ollama for the end-to-end tests: lists one model and answers /api/generate
# with a canned summary in the shape minutes-core asks for. Usage: fake-ollama.py PORT LOGFILE
# While LOGFILE.slow exists, each answer takes 6 s (to see what the app shows meanwhile).
# While LOGFILE.fail exists, each answer is an HTTP 500 (to see a failed summary).
import json, os, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
SUMMARY = "- The team reviewed the onboarding launch plan.\n- Download size is the main risk; a Fast model will be the default.\n\n## Action Items\n- [ ] Build the settings screen (by Friday)\n"
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        body = json.dumps({"models": [{"name": "fake:latest"}]}).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json"); self.end_headers(); self.wfile.write(body)
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0)); req = json.loads(self.rfile.read(n) or b"{}")
        open(sys.argv[2], "a").write(json.dumps({"path": self.path, "model": req.get("model"), "prompt_chars": len(req.get("prompt", "")), "system": req.get("system", ""), "prompt": req.get("prompt", "")}) + "\n")
        if os.path.exists(sys.argv[2] + ".slow"): time.sleep(6)
        if os.path.exists(sys.argv[2] + ".fail"):
            self.send_response(500); self.end_headers(); self.wfile.write(b'{"error": "fake failure"}'); return
        body = json.dumps({"model": req.get("model"), "response": SUMMARY, "done": True}).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json"); self.end_headers(); self.wfile.write(body)
ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
