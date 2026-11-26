#!/usr/bin/env python3
"""Run fixed AETHER CODE / OpenCode coding tasks in disposable repositories.

Every run gets a new repository and HOME under /tmp. The AETHER1_CODE_EVAL_DB
override keeps the development binary away from the operator's real settings.
Results and the fixture are retained under the printed output directory.
"""

import argparse
import ipaddress
import json
import os
from pathlib import Path
import platform
import shutil
import socket
import socketserver
import sqlite3
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlparse


CASES = {
    "small": {
        "prompt": "Implement normalize_tag in solution.py. Strip surrounding whitespace, lowercase ASCII letters, collapse each run of spaces or underscores to one hyphen, and reject an empty result with ValueError. Run the tests.",
        "tests": '''import unittest
from solution import normalize_tag

class TestTags(unittest.TestCase):
    def test_words(self):
        self.assertEqual(normalize_tag("  Hello__World  "), "hello-world")
        self.assertEqual(normalize_tag("a   B"), "a-b")
    def test_empty(self):
        with self.assertRaises(ValueError):
            normalize_tag(" _  ")
''',
        "solution": "def normalize_tag(text):\n    raise NotImplementedError\n",
    },
    "medium": {
        "prompt": "Implement parse_duration in solution.py. Accept one or more non-repeated positive integer units h, m, s in descending order (examples: 2h30m, 45s, 1h2m3s). Return seconds. Reject zero, missing units, repeats, wrong order, junk, and whitespace with ValueError. Run the tests.",
        "tests": '''import unittest
from solution import parse_duration

class TestDuration(unittest.TestCase):
    def test_values(self):
        self.assertEqual(parse_duration("2h30m"), 9000)
        self.assertEqual(parse_duration("45s"), 45)
        self.assertEqual(parse_duration("1h2m3s"), 3723)
    def test_invalid(self):
        for value in ("", "0s", "2m1h", "1h1h", "4", " 1s", "1s ", "1x"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                parse_duration(value)
''',
        "solution": "def parse_duration(text):\n    raise NotImplementedError\n",
    },
    "long": {
        "prompt": "Implement build_order in solution.py. Input is a dict mapping each job name to its prerequisite names. Return a deterministic topological order: whenever several jobs are ready, choose the alphabetically first. Reject unknown prerequisites and cycles with ValueError. Do not mutate the input. Run the tests.",
        "tests": '''import unittest
from solution import build_order

class TestBuildOrder(unittest.TestCase):
    def test_order(self):
        jobs = {"deploy": ["test", "package"], "test": ["build"], "package": ["build"], "build": []}
        original = {name: list(needs) for name, needs in jobs.items()}
        self.assertEqual(build_order(jobs), ["build", "package", "test", "deploy"])
        self.assertEqual(jobs, original)
    def test_independent(self):
        self.assertEqual(build_order({"z": [], "a": [], "m": []}), ["a", "m", "z"])
    def test_invalid(self):
        with self.assertRaises(ValueError): build_order({"a": ["missing"]})
        with self.assertRaises(ValueError): build_order({"a": ["b"], "b": ["a"]})
''',
        "solution": "def build_order(jobs):\n    raise NotImplementedError\n",
    },
}


def command(args, *, cwd=None, env=None, timeout=30):
    return subprocess.run(args, cwd=cwd, env=env, text=True, capture_output=True, timeout=timeout)


def prepare(root, case):
    root.mkdir(parents=True)
    work = root / "workspace"
    work.mkdir()
    (work / "solution.py").write_text(CASES[case]["solution"])
    (work / "test_solution.py").write_text(CASES[case]["tests"])
    (work / "README.md").write_text(CASES[case]["prompt"] + "\n")
    for args in (["git", "init", "-q"], ["git", "add", "-A"],
                 ["git", "-c", "user.name=Evaluation", "-c", "user.email=eval@example.invalid", "commit", "-qm", "fixture"]):
        result = command(args, cwd=work)
        if result.returncode:
            raise RuntimeError(result.stderr.strip())
    return work


def set_setting(db, key, value):
    with sqlite3.connect(db) as conn:
        conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
                     (key, json.dumps(value)))


def run_aether(binary, home, work, model, endpoint, prompt):
    source = shutil.which("opencode")
    if not source:
        raise RuntimeError("OpenCode binary not found; install it before evaluating Aether1")
    shutil.copy2(source, home.parent / "opencode")
    env = dict(HOME=str(home), PATH=f"{home.parent}:/usr/local/bin:/usr/bin:/bin",
               AETHER1_CODE_EVAL_DB=str(home / "eval.db"), AETHER1_LOCAL_ONLY="1")
    setup = command([binary, "code", "workspace", str(work)], env=env)
    if setup.returncode:
        return setup
    for key, value in (("llm_provider", "ollama"), ("llm_endpoint", endpoint),
                       ("llm_model", model), ("code_model_preference", model)):
        set_setting(home / "eval.db", key, value)
    level = command([binary, "code", "level", "developer"], env=env)
    if level.returncode:
        return level
    return command([binary, "code", "do", prompt], env=env, timeout=900)


def run_direct(binary, home, work, model, endpoint, prompt):
    if platform.system() != "Linux":
        raise RuntimeError("the direct baseline requires Linux Bubblewrap confinement")
    config = home / ".config" / "opencode"
    config.mkdir(parents=True)
    base = "http://127.0.0.1:8081/v1"
    (config / "opencode.json").write_text(json.dumps({
        "provider": {"aether1-local": {"npm": "@ai-sdk/openai-compatible",
            "name": "Evaluation local model", "options": {"baseURL": base},
            "models": {model: {"name": model}}}}
    }))
    env = dict(HOME=str(home), PATH="/usr/local/bin:/usr/bin:/bin",
               XDG_CONFIG_HOME=str(home / ".config"),
               XDG_DATA_HOME=str(home / ".local" / "share"),
               XDG_STATE_HOME=str(home / ".local" / "state"),
               XDG_CACHE_HOME=str(home / ".cache"))
    source = shutil.which(binary)
    if not source:
        raise RuntimeError(f"OpenCode binary not found: {binary}")
    confined_binary = home.parent / "opencode-eval"
    shutil.copy2(source, confined_binary)
    host_home = Path.home()
    if not shutil.which("bwrap"):
        raise RuntimeError("bubblewrap is required for the direct baseline")
    parsed = urlparse(endpoint)
    if parsed.scheme != "http":
        raise RuntimeError("direct baseline currently supports local HTTP model endpoints")
    socket_path = home.parent / "model.sock"
    child = home.parent / "model_relay.py"
    child.write_text('''import socket, subprocess, sys, threading
def pipe(a, b):
    try:
        while data := a.recv(65536): b.sendall(data)
    except OSError: pass
    try: b.shutdown(socket.SHUT_WR)
    except OSError: pass
def handle(client):
    try:
        peer = socket.socket(socket.AF_UNIX)
        peer.connect(sys.argv[1])
        threading.Thread(target=pipe, args=(client, peer), daemon=True).start()
        pipe(peer, client)
    finally: client.close()
server = socket.socket()
server.bind(("127.0.0.1", 8081))
server.listen(32)
def accept():
    while True:
        client, _ = server.accept()
        threading.Thread(target=handle, args=(client,), daemon=True).start()
threading.Thread(target=accept, daemon=True).start()
sys.exit(subprocess.run(sys.argv[2:]).returncode)
''')
    class Relay(socketserver.BaseRequestHandler):
        def handle(self):
            try:
                upstream = socket.create_connection((parsed.hostname, parsed.port or 80), timeout=10)
            except OSError:
                return
            def pipe(a, b):
                try:
                    while data := a.recv(65536):
                        b.sendall(data)
                except OSError:
                    pass
                try:
                    b.shutdown(socket.SHUT_WR)
                except OSError:
                    pass
            thread = threading.Thread(target=pipe, args=(self.request, upstream), daemon=True)
            thread.start()
            pipe(upstream, self.request)
            upstream.close()
            thread.join(timeout=2)
    class Server(socketserver.ThreadingUnixStreamServer):
        daemon_threads = True
    try:
        relay_server = Server(str(socket_path), Relay)
    except OSError as error:
        raise RuntimeError(f"cannot create the local model relay: {error}") from error
    with relay_server as relay:
        thread = threading.Thread(target=relay.serve_forever, daemon=True)
        thread.start()
        wrapped = ["bwrap", "--die-with-parent", "--unshare-user", "--unshare-pid",
               "--unshare-ipc", "--unshare-uts", "--unshare-net", "--ro-bind", "/", "/",
               "--tmpfs", str(host_home), "--tmpfs", "/run", "--tmpfs", "/tmp",
               "--bind", str(home.parent), str(home.parent),
               "--dev-bind", "/dev", "/dev", "--proc", "/proc",
               "--chdir", str(work), "--", "python3", str(child), str(socket_path),
               str(confined_binary), "run", "--format", "json",
               "--model", f"aether1-local/{model}", prompt]
        try:
            return command(wrapped, cwd=work, env=env, timeout=900)
        finally:
            relay.shutdown()
            thread.join(timeout=2)


def evaluate(work):
    result = command(["python3", "-m", "unittest", "-q", "test_solution"], cwd=work)
    return {"passed": result.returncode == 0, "output": (result.stdout + result.stderr)[-4000:]}


def local_endpoint(value):
    parsed = urlparse(value)
    if parsed.scheme not in ("http", "https") or not parsed.hostname:
        return False
    if parsed.hostname == "localhost":
        return True
    try:
        return ipaddress.ip_address(parsed.hostname).is_private
    except ValueError:
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", choices=CASES, required=True)
    parser.add_argument("--mode", choices=("aether", "direct"), required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--model-version", default="unspecified", help="Local model digest or release")
    parser.add_argument("--endpoint", default="http://127.0.0.1:11434")
    parser.add_argument("--aether-bin", default="src-tauri/target/debug/aether1")
    parser.add_argument("--opencode-bin", default="opencode")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    if not local_endpoint(args.endpoint):
        parser.error("--endpoint must name localhost or a private IP address")
    root = args.out or Path(tempfile.mkdtemp(prefix="aether1-code-eval-"))
    root = root.resolve()
    if root.exists() and any(root.iterdir()):
        parser.error(f"output directory is not empty: {root}")
    work = prepare(root, args.case)
    home = root / "home"
    home.mkdir()
    binary = args.aether_bin if args.mode == "aether" else args.opencode_bin
    resolved = Path(binary).resolve() if "/" in binary else binary
    version = command([str(resolved), "--version"])
    started = time.monotonic()
    setup_error = None
    try:
        if args.mode == "aether":
            result = run_aether(str(resolved), home, work, args.model, args.endpoint, CASES[args.case]["prompt"])
        else:
            result = run_direct(str(resolved), home, work, args.model, args.endpoint, CASES[args.case]["prompt"])
        exit_code, output = result.returncode, result.stdout + result.stderr
    except (OSError, subprocess.TimeoutExpired, RuntimeError) as error:
        exit_code, output = 124, str(error)
        setup_error = str(error)
    elapsed = round(time.monotonic() - started, 2)
    score = evaluate(work)
    blocked = bool(setup_error or "No coding model is downloaded yet" in output
                   or ("bwrap:" in output and "Operation not permitted" in output))
    outcome = "pass" if score["passed"] else "blocked" if blocked else "fail"
    report = {"case": args.case, "mode": args.mode, "model": args.model,
              "model_version": args.model_version,
              "engine_version": ((version.stdout.strip() or version.stderr.strip()).splitlines() or ["unknown"])[0],
              "machine": platform.platform(), "prompt": CASES[args.case]["prompt"],
              "elapsed_seconds": elapsed, "exit_code": exit_code,
              "tests": score, "outcome": outcome, "setup_error": setup_error,
              "human_interventions": 0, "recovery_used": False,
              "workspace": str(work)}
    (root / "agent-output.txt").write_text(output)
    (root / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(root)
    print(f"{args.mode} / {args.case}: {outcome.upper()}; agent exit {exit_code}; {elapsed}s")
    return 0 if score["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
