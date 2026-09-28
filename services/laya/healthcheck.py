"""Docker healthcheck + watchdog for laya-serve.

laya-serve has been seen deadlocking under concurrent requests: the port still accepts
connections, but no request (not even GET /health) is ever answered. Docker only
restarts containers that *exit*, never ones that are merely unhealthy, so a hung server
would stay down until someone noticed.

This check tries GET /health a few times. If the server answers, the container is
healthy. If it never answers, it kills the laya-serve process so the container exits
and `restart: unless-stopped` brings it back. Several attempts with generous timeouts
keep a busy-but-alive server (a slow CPU inference) from being killed.

Once healthy it also sends one warm-up inference per laya-serve process: checkpoints
load lazily (LAYA_PRELOAD=0), so otherwise the first real hallucination check after
every (re)start pays the model load and overruns its timeout.

Requires `init: true` on the service: Linux ignores signals (even SIGKILL) sent to a
container's PID 1 from inside the container, so laya-serve must not be PID 1.
"""

import json
import os
import signal
import sys
import time
import urllib.request

URL = "http://127.0.0.1:8000/health"
ATTEMPTS = 3
TIMEOUT_S = 10
PAUSE_S = 2
NUL = bytes([0])


def laya_serve_pids() -> list:
    """PIDs of laya-serve processes (never this healthcheck itself)."""
    pids = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit() or int(entry) == os.getpid():
            continue
        try:
            with open(f"/proc/{entry}/cmdline", "rb") as f:
                cmdline = f.read().replace(NUL, b" ").decode(errors="replace")
        except OSError:
            continue
        # Skip the init process (`docker-init -- laya-serve`) and this script.
        if "laya-serve" in cmdline and "docker-init" not in cmdline and "healthcheck" not in cmdline:
            pids.append(int(entry))
    return sorted(pids)


def process_start_time(pid: int) -> str:
    """Kernel start time of `pid` (field 22 of /proc/<pid>/stat). PIDs repeat across
    container restarts (a fresh namespace starts counting again) and /tmp survives a
    restart, so the warm-up marker must include the start time, not just the PID."""
    try:
        with open(f"/proc/{pid}/stat") as f:
            return f.read().rsplit(")", 1)[1].split()[19]
    except (OSError, IndexError):
        return "unknown"


def warm_up_once() -> None:
    """Load the model with one tiny inference, once per laya-serve process."""
    pids = laya_serve_pids()
    if not pids:
        return
    marker = f"/tmp/laya-warm-{pids[0]}-{process_start_time(pids[0])}"
    if os.path.exists(marker):
        return
    body = {
        "state": {"response": "Paris is the capital of France.", "prompt": "What is the capital of France?"},
        "questions": {"warmup": {"type": "choice", "instructions": "Is this a warm-up?",
                                 "criteria": {"A": "no", "B": "yes"}}},
    }
    request = urllib.request.Request(
        "http://127.0.0.1:8000/v1/systemone",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
    )
    try:
        urllib.request.urlopen(request, timeout=30).read()
        open(marker, "w").close()
    except Exception as exc:  # retried on the next healthcheck; never fails health
        print(f"warm-up failed (will retry): {exc}", file=sys.stderr)


def main() -> int:
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(URL, timeout=TIMEOUT_S) as response:
                if response.status == 200:
                    warm_up_once()
                    return 0
        except Exception as exc:  # timeout, connection refused, HTTP error
            print(f"attempt {attempt}/{ATTEMPTS}: {exc}", file=sys.stderr)
        if attempt < ATTEMPTS:
            time.sleep(PAUSE_S)

    pids = laya_serve_pids()
    print(f"laya-serve unresponsive; killing {pids} so the container restarts", file=sys.stderr)
    for pid in pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError as exc:
            print(f"kill {pid}: {exc}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
