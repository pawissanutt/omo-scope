#!/usr/bin/env python3
"""Record the README demo against a fabricated OmO project (no real session data).

    python3 demo/record.py target/release/omo-scope demo/omo-scope.cast
    agg --theme monokai --font-size 15 demo/omo-scope.cast demo/omo-scope.gif
"""

import codecs
import fcntl
import json
import os
import pty
import select
import shutil
import struct
import sys
import termios
import time
from datetime import datetime, timedelta, timezone

W, H = 100, 30
QUIT = 24.0
BIN, CAST = os.path.abspath(sys.argv[1]), sys.argv[2]
BASE = "/tmp/omo-demo"
PROJ = f"{BASE}/acme-api"
AGENT = f"{BASE}/agent"
STORE = f"{PROJ}/.omo/senpi-task"
SESSION = "demo-login-limits"


def iso(ago=0.0):
    t = datetime.now(timezone.utc) - timedelta(seconds=ago)
    return t.isoformat(timespec="milliseconds").replace("+00:00", "Z")


def put(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path + ".tmp", "w") as f:
        json.dump(obj, f)
    os.replace(path + ".tmp", path)


def append(path, *objs):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "a") as f:
        f.writelines(json.dumps(o) + "\n" for o in objs)


def append_text(path, line):
    with open(path, "a") as f:
        f.write(line + "\n")


def msg(role, content, ago=0.0, **extra):
    return {"type": "message", "timestamp": iso(ago), "message": {"role": role, "content": content, **extra}}


def assistant(*blocks, ago=0.0, stop="toolUse"):
    return msg("assistant", list(blocks), ago, stopReason=stop)


def think(t):
    return {"type": "thinking", "thinking": t}


def say(t):
    return {"type": "text", "text": t}


def call(cid, name, **args):
    return {"type": "toolCall", "id": cid, "name": name, "arguments": args}


def result(cid, name, text, ago=0.0):
    return msg("toolResult", [say(text)], ago, toolCallId=cid, toolName=name, isError=False)


def header(tid):
    return {"type": "session", "version": 3, "id": f"cs-{tid}", "cwd": PROJ}


def transcript(tid):
    return f"{STORE}/children/{tid}/sessions/{tid}/2026-10-02T09-00-00.000Z_{tid}.jsonl"


def session_file(sid, title, hours_ago):
    path = f"{AGENT}/sessions/--{PROJ.strip('/').replace('/', '-')}--/2026-10-02T09-00-00-000Z_{sid}.jsonl"
    append(path, {"type": "session", "version": 3, "id": sid, "cwd": PROJ}, {"type": "session_info", "name": title})
    t = time.time() - hours_ago * 3600
    os.utime(path, (t, t))


def task(tid, summary, category, model, status, created, ended=None, stats=(0, 0, 0)):
    t = {"task_id": tid, "status": status, "parent_session_id": SESSION, "root_session_id": SESSION,
         "created_at": iso(created), "started_at": iso(created), "updated_at": iso(ended or 0),
         "task_summary": summary, "category": category, "resolved_model": {"model_id": model},
         "run_stats": dict(zip(("turns", "tool_calls", "total_tokens"), stats))}
    if ended is not None:
        t["terminal_at"] = iso(ended)
    put(f"{STORE}/tasks/{tid}.json", t)


def dag(build_state, review_state, review_task=None):
    nodes = [("scout", "Map the auth middleware and request hooks", "completed", [], "st_scout"),
             ("build", "Implement a token-bucket limiter on /login", build_state, ["scout"], "st_build"),
             ("review", "Review the limiter and run the full suite", review_state, ["build"], review_task)]
    put(f"{STORE}/dag/runs/dag_demo.json", {
        "runId": "dag_demo", "name": "Login rate limiting", "status": "running",
        "rootSessionId": SESSION, "parentSessionId": SESSION, "updatedAt": iso(),
        "definition": {"nodes": [{"id": n[0], "task_summary": n[1]} for n in nodes]},
        "nodes": [{"id": i, "state": s, "dependsOn": d, **({"taskId": t} if t else {})} for i, _, s, d, t in nodes]})


def fixture():
    shutil.rmtree(BASE, ignore_errors=True)
    os.makedirs(f"{PROJ}/target")
    session_file(SESSION, "Add rate limiting to the login API", 0)
    session_file("demo-flaky-e2e", "Fix the flaky checkout e2e test", 3)
    session_file("demo-axum", "Upgrade the server to axum 0.9", 26)
    task("st_scout", "Map the auth middleware and request hooks", "explore", "claude-sonnet-5", "completed", 300, 200,
         (4, 6, 41000))
    task("st_build", "Implement a token-bucket limiter on /login", "deep", "gpt-6-sol", "running", 190, None,
         (7, 9, 88000))
    dag("running", "pending")
    append(transcript("st_scout"), header("st_scout"),
           msg("user", "Find where POST /login is handled and where a rate limiter could hook in.", 300),
           assistant(think("Login should be routed in src/routes and middleware lives in src/middleware. Grep first."),
                     call("s1", "grep", pattern="login", path="src"), ago=296),
           result("s1", "grep", 'src/routes/auth.rs:14:    .route("/login", post(login))\nsrc/middleware/mod.rs:3: pub mod session;', 295),
           assistant(call("s2", "read", path="src/routes/auth.rs"), ago=290),
           result("s2", "read", 'pub fn router() -> Router {\n    Router::new()\n        .route("/login", post(login))\n'
                  "        .layer(session::layer())\n}", 289),
           assistant(say("POST /login is wired in src/routes/auth.rs behind the session layer. A tower layer keyed by "
                         "client IP, added before session::layer(), covers it without touching the handler."),
                     ago=205, stop="stop"))
    build = transcript("st_build")
    append(build, header("st_build"),
           msg("user", "Add a token-bucket limiter to POST /login: 5 attempts per minute per IP, 429 with Retry-After.", 190),
           assistant(think("Per-IP buckets need shared state. A Mutex<HashMap> is enough at this scale; refilling lazily on "
                           "each request avoids a timer task, so idle IPs cost nothing."),
                     say("I'll add a tower layer with lazily refilled per-IP buckets."),
                     call("b1", "read", path="src/middleware/mod.rs"), ago=170),
           result("b1", "read", "pub mod session;", 168))
    return build


def finish(build):
    append(build, assistant(say("Done: POST /login allows 5 requests per minute per IP, then answers 429 with "
                                "Retry-After. All 6 limiter tests pass."), stop="stop"))
    task("st_build", "Implement a token-bucket limiter on /login", "deep", "gpt-6-sol", "completed", 190, 0,
         (9, 12, 112000))
    task("st_review", "Review the limiter and run the full suite", "review", "claude-opus-5-5", "running", 0)
    append(transcript("st_review"), header("st_review"),
           msg("user", "Review the rate limiter change and run the whole test suite."))
    dag("completed", "running", "st_review")


def click(row, col=14):
    return f"\x1b[<0;{col};{row}M\x1b[<0;{col};{row}m"


def timeline(build, send):
    log = f"{PROJ}/target/limiter.log"
    tests = ["allows_burst_of_five", "sixth_request_gets_429", "retry_after_header_is_set",
             "refills_after_a_minute", "buckets_are_per_ip"]
    lines = ["   Compiling acme-api v0.4.0 (/tmp/omo-demo/acme-api)",
             "    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.21s",
             "     Running unittests src/lib.rs (target/debug/deps/acme_api-3f9c2d)", "", "running 6 tests",
             *[f"test middleware::rate_limit::tests::{t} ... ok" for t in tests],
             "test routes::auth::tests::login_still_succeeds ... ok", "",
             "test result: ok. 6 passed; 0 failed; 0 ignored; finished in 0.04s"]
    cmd = f"cd {PROJ} && cargo test limiter > target/limiter.log 2>&1"
    steps = [
        (1.0, lambda: append(build, assistant(think("Write the layer first, then put it in front of the session layer."),
                                              call("b2", "edit", path="src/middleware/rate_limit.rs")))),
        (2.2, lambda: append(build, result("b2", "edit", "Created src/middleware/rate_limit.rs (84 lines)"))),
        (3.4, lambda: send(click(3))),
        (4.8, lambda: send("x")),
        (6.4, lambda: send("x")),
        (7.2, lambda: send(click(4))),
        (8.0, lambda: append(build, assistant(say("Wired into the router. Running the limiter tests."),
                                              call("b3", "bash", command=cmd)))),
        (14.8, lambda: send("\x1b[<0;40;6M")),
        (15.0, lambda: send("\x1b[<32;40;8M")),
        (15.2, lambda: send("\x1b[<32;40;10M")),
        (15.4, lambda: send("\x1b[<0;40;10m")),
        (16.4, lambda: append(build, result("b3", "bash", "test result: ok. 6 passed; 0 failed"))),
        (17.2, lambda: finish(build)),
        (19.0, lambda: send("s")),
        (20.2, lambda: send("j")),
        (21.0, lambda: send("j")),
        (22.4, lambda: send("s")),
        (QUIT, lambda: send("q")),
    ]
    steps += [(8.8 + n * 0.42, lambda line=line: append_text(log, line)) for n, line in enumerate(lines)]
    return sorted(steps, key=lambda s: s[0])


def main():
    build = fixture()
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", H, W, 0, 0))
        env = {k: v for k, v in os.environ.items() if not k.startswith(("PI_", "HERDR_", "SENPI_", "OMO_", "NO_COLOR"))}
        env.update(TERM="xterm-256color", OMO_CODING_AGENT_DIR=AGENT)
        os.execve(BIN, [BIN, "--cwd", PROJ, "--session", SESSION], env)
    steps = timeline(build, lambda s: os.write(fd, s.encode()))
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    events, start = [], time.monotonic()
    while True:
        t = time.monotonic() - start
        while steps and steps[0][0] <= t:
            steps.pop(0)[1]()
        if not select.select([fd], [], [], 0.02)[0]:
            continue
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        if t < QUIT:
            events.append([round(t, 3), "o", decoder.decode(data)])
    os.waitpid(pid, 0)
    with open(CAST, "w") as f:
        f.write(json.dumps({"version": 2, "width": W, "height": H, "env": {"TERM": "xterm-256color"}}) + "\n")
        f.writelines(json.dumps(e) + "\n" for e in events)
    shutil.rmtree(BASE, ignore_errors=True)


main()
