#!/usr/bin/env python3
"""N5 resident soak and recovery harness.

Field mode drives one already-admitted QQ generation through a single bridge
attach, repeated OneBot health queries, and controlled core crash/restart
injections. Rehearsal mode exercises the same daemon start/restart/stop path
without QQ or a bridge. The harness never sends a message and never hot-unloads
a mapped bridge.

Raw state, tokens, logs, journal and samples stay in the ignored pack
directory. Only a summary is meant to be copied into acceptance evidence.
"""

from __future__ import annotations

import argparse
import ctypes
import datetime as dt
import hashlib
import json
import os
import statistics
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

CREATE_NO_WINDOW = 0x08000000
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
STILL_ACTIVE = 259

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)


class FILETIME(ctypes.Structure):
    _fields_ = [("low", ctypes.c_uint32), ("high", ctypes.c_uint32)]


kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
kernel32.OpenProcess.restype = ctypes.c_void_p
kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
kernel32.CloseHandle.restype = ctypes.c_int
kernel32.GetProcessHandleCount.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
kernel32.GetProcessHandleCount.restype = ctypes.c_int
kernel32.GetExitCodeProcess.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
kernel32.GetExitCodeProcess.restype = ctypes.c_int
kernel32.GetProcessTimes.argtypes = [
    ctypes.c_void_p,
    ctypes.POINTER(FILETIME),
    ctypes.POINTER(FILETIME),
    ctypes.POINTER(FILETIME),
    ctypes.POINTER(FILETIME),
]
kernel32.GetProcessTimes.restype = ctypes.c_int


class PROCESS_MEMORY_COUNTERS_EX(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_uint32),
        ("PageFaultCount", ctypes.c_uint32),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
        ("PrivateUsage", ctypes.c_size_t),
    ]


psapi.GetProcessMemoryInfo.argtypes = [
    ctypes.c_void_p,
    ctypes.POINTER(PROCESS_MEMORY_COUNTERS_EX),
    ctypes.c_uint32,
]
psapi.GetProcessMemoryInfo.restype = ctypes.c_int


def utcnow() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_prefix(path: Path, length: int) -> str:
    with path.open("rb") as handle:
        return hashlib.sha256(handle.read(length)).hexdigest()


def complete_frame_prefix(path: Path):
    """Return (complete_bytes, sha256(complete_bytes)) for the journal prefix.

    A core kill can catch the writer in the middle of a frame. Recovery may then
    truncate that partial tail; comparing only complete frames avoids treating a
    safe truncation as a changed prefix.
    """
    data = path.read_bytes()
    offset = 0
    while offset + 16 <= len(data):
        if data[offset : offset + 4] != b"CLGJ":
            break
        version, length, _crc = struct.unpack("<III", data[offset + 4 : offset + 16])
        if version != 1 or offset + 16 + length > len(data) or length > 16 * 1024 * 1024:
            break
        offset += 16 + length
    return offset, hashlib.sha256(data[:offset]).hexdigest()


def new_hex(byte_count: int) -> str:
    return os.urandom(byte_count).hex()


def free_port() -> int:
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def process_metrics(pid: int):
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
    if not handle:
        return {"available": False, "error": ctypes.get_last_error()}
    try:
        counters = PROCESS_MEMORY_COUNTERS_EX()
        counters.cb = ctypes.sizeof(counters)
        memory_ok = bool(psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb))
        handles = ctypes.c_uint32(0)
        handles_ok = bool(kernel32.GetProcessHandleCount(handle, ctypes.byref(handles)))
        creation, exit_time, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
        times_ok = bool(
            kernel32.GetProcessTimes(
                handle,
                ctypes.byref(creation),
                ctypes.byref(exit_time),
                ctypes.byref(kernel),
                ctypes.byref(user),
            )
        )
        cpu_100ns = (
            ((kernel.high << 32) | kernel.low) + ((user.high << 32) | user.low)
            if times_ok
            else 0
        )
        exit_code = ctypes.c_uint32(0)
        alive = bool(kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code))) and exit_code.value == STILL_ACTIVE
        return {
            "available": True,
            "alive": alive,
            "working_set_bytes": counters.WorkingSetSize if memory_ok else None,
            "peak_working_set_bytes": counters.PeakWorkingSetSize if memory_ok else None,
            "private_bytes": counters.PrivateUsage if memory_ok else None,
            "pagefile_bytes": counters.PagefileUsage if memory_ok else None,
            "handles": handles.value if handles_ok else None,
            "cpu_seconds": round(cpu_100ns / 10_000_000, 3) if times_ok else None,
        }
    finally:
        kernel32.CloseHandle(handle)


def powershell(script: str, timeout: float = 20.0) -> subprocess.CompletedProcess:
    return subprocess.run(
        [
            "C:\\windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def process_identity(pid: int):
    result = powershell(
        f"$p=Get-Process -Id {pid}; if(-not $p){{exit 3}}; "
        "Write-Output ($p.StartTime.ToUniversalTime().ToString('o') + '|' + $p.Responding + '|' + $p.MainWindowHandle)"
    )
    if result.returncode != 0:
        return None
    parts = result.stdout.strip().split("|")
    if len(parts) != 3:
        return None
    return {"created": parts[0], "responding": parts[1] == "True", "window": int(parts[2])}


def bridge_mapped(pid: int, bridge: Path) -> bool:
    expected = str(bridge).replace("'", "''")
    result = powershell(
        f"$p=Get-Process -Id {pid}; if(-not $p){{exit 3}}; "
        f"@($p.Modules | Where-Object {{ $_.FileName -eq '{expected}' }}).Count"
    )
    return result.returncode == 0 and result.stdout.strip().isdigit() and int(result.stdout.strip()) == 1


def onebot_action(port: int, token: str, action: str, params=None, timeout: float = 10.0):
    body = json.dumps(
        {"action": action, "params": params or {}, "echo": f"{action}-{uuid.uuid4().hex[:8]}"}
    ).encode("utf-8")
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/",
        data=body,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read()
    return json.loads(raw)


def probe(port: int, token: str, action: str, params=None):
    started = time.perf_counter()
    try:
        reply = onebot_action(port, token, action, params)
        latency = round((time.perf_counter() - started) * 1000, 2)
        return {
            "ok": reply.get("retcode") == 0,
            "retcode": reply.get("retcode"),
            "status": reply.get("status"),
            "latency_ms": latency,
            "data": reply.get("data"),
        }
    except Exception as error:  # noqa: BLE001 - field evidence records the failure text
        return {
            "ok": False,
            "retcode": None,
            "error": f"{type(error).__name__}: {error}",
            "latency_ms": round((time.perf_counter() - started) * 1000, 2),
        }


def sanitize_probe(value):
    if not isinstance(value, dict):
        return value
    keep = ("ok", "retcode", "status", "error", "latency_ms")
    return {key: value[key] for key in keep if key in value}


def wait_ready(log_path: Path, proc: subprocess.Popen, timeout: float = 20.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if log_path.exists():
            text = log_path.read_text(encoding="utf-8", errors="replace").splitlines()
            if text:
                try:
                    value = json.loads(text[0])
                    if value.get("onebot_http"):
                        return value
                except json.JSONDecodeError:
                    pass
        if proc.poll() is not None:
            break
        time.sleep(0.1)
    return None


class Soak:
    def __init__(self, state: dict, pack: Path, args):
        self.state = state
        self.pack = pack
        self.args = args
        self.out = Path(args.out) if args.out else pack
        self.out.mkdir(parents=True, exist_ok=True)
        self.events_path = self.out / "soak-events.jsonl"
        self.events = self.events_path.open("a", encoding="utf-8")
        self.daemon_proc: subprocess.Popen | None = None
        self.ready: dict | None = None
        self.started = time.time()
        self.start_utc = utcnow()
        self.samples: list[dict] = []
        self.restarts: list[dict] = []
        self.probe_failures: list[dict] = []
        self.stop_record: dict = {}
        self.fatal: str | None = None
        self.bridge_initial: bool | None = None
        self.bridge_final: bool | None = None
        self.journal_initial: dict | None = None
        self.journal_final: dict | None = None

    def log_event(self, kind: str, **fields):
        record = {"t_utc": utcnow(), "kind": kind, "elapsed_s": round(time.time() - self.started, 2)}
        record.update(fields)
        self.events.write(json.dumps(record, ensure_ascii=False) + "\n")
        self.events.flush()
        print(json.dumps(record, ensure_ascii=False), flush=True)

    def daemon_args(self, account: str):
        return [
            str(self.state["build"]["caligo_exe"]),
            "daemon",
            "--pipe-prefix",
            self.state["pipe"],
            "--journal",
            self.state["journal"],
            "--baseline",
            "qq-9.9.33-52230",
            "--account",
            account,
            "--core-build",
            "caligo-n5-soak",
            "--token-file",
            self.state["token_file"],
            "--onebot-http-bind",
            f"127.0.0.1:{self.state['ports']['http']}",
            "--onebot-ws-bind",
            f"127.0.0.1:{self.state['ports']['ws']}",
        ]

    def start_daemon(self, index: int, expect_pid: int | None):
        args = self.daemon_args(str(self.state["account"]))
        if expect_pid:
            args += ["--expect-pid", str(expect_pid)]
        log_path = self.out / f"daemon-{index}.stdout.private.log"
        err_path = self.out / f"daemon-{index}.stderr.private.log"
        log_path.unlink(missing_ok=True)
        err_path.unlink(missing_ok=True)
        with log_path.open("wb") as out, err_path.open("wb") as err:
            proc = subprocess.Popen(
                args,
                cwd=str(Path(args[0]).parent),
                stdout=out,
                stderr=err,
                creationflags=CREATE_NO_WINDOW,
            )
        ready = wait_ready(log_path, proc, timeout=30.0)
        if not ready:
            if proc.poll() is None:
                proc.kill()
            error = err_path.read_text(encoding="utf-8", errors="replace") if err_path.exists() else ""
            raise RuntimeError(f"daemon start failed (exit={proc.poll()}): {error[-2000:]}")
        self.daemon_proc = proc
        self.ready = ready
        write_json(
            self.pack / "daemon.private.json",
            {
                "pid": proc.pid,
                "started_utc": utcnow(),
                "bridge_pipe": ready.get("bridge_pipe"),
                "control_pipe": ready.get("control_pipe"),
                "onebot_http": ready.get("onebot_http"),
                "onebot_ws": ready.get("onebot_ws"),
            },
        )
        self.log_event("daemon_started", pid=proc.pid, http=ready.get("onebot_http"), ws=ready.get("onebot_ws"))
        return proc, ready

    def wait_login(self, timeout: float = 120.0):
        deadline = time.time() + timeout
        last = None
        while time.time() < deadline:
            last = probe(self.state["ports"]["http"], self.token, "get_login_info")
            if last.get("ok") and str((last.get("data") or {}).get("user_id")) == str(self.state["account"]):
                return last
            time.sleep(1.0)
        return last

    def stop_daemon(self, force: bool = False):
        if not self.daemon_proc:
            return
        if self.daemon_proc.poll() is None:
            result = subprocess.run(
                [
                    str(self.state["build"]["caligo_exe"]),
                    "cli",
                    "daemon-control",
                    "--pipe",
                    self.ready["control_pipe"],
                    "--auth",
                    self.ready["auth_token"],
                    "stop",
                ],
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.stop_record = {
                "control_exit": result.returncode,
                "stdout": result.stdout.strip(),
                "stderr": result.stderr.strip(),
                "method": "control-stop",
            }
            if result.returncode != 0 and force:
                self.daemon_proc.kill()
                self.stop_record["method"] = "control-stop-failed-force-kill"
        try:
            self.daemon_proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            if force:
                self.daemon_proc.kill()
                self.daemon_proc.wait(timeout=10)
            else:
                raise
        self.stop_record["exited"] = self.daemon_proc.poll() is not None
        self.stop_record["exit_code"] = self.daemon_proc.poll()

    def journal_state(self, with_hash: bool):
        path = Path(self.state["journal"])
        if not path.exists():
            return {"exists": False}
        value = {"exists": True, "size": path.stat().st_size, "mtime_utc": dt.datetime.fromtimestamp(path.stat().st_mtime, dt.timezone.utc).isoformat()}
        if with_hash:
            value["sha256"] = sha256_file(path)
            complete_bytes, complete_hash = complete_frame_prefix(path)
            value["complete_bytes"] = complete_bytes
            value["complete_sha256"] = complete_hash
        return value

    def sample(self, index: int, with_hash: bool):
        version = probe(self.state["ports"]["http"], self.token, "get_version_info")
        status = probe(self.state["ports"]["http"], self.token, "get_status")
        login = probe(self.state["ports"]["http"], self.token, "get_login_info")
        record = {
            "t_utc": utcnow(),
            "elapsed_s": round(time.time() - self.started, 2),
            "daemon": process_metrics(self.daemon_proc.pid) if self.daemon_proc else {"available": False},
            "qq": process_metrics(int(self.state["pid"])) if self.state.get("pid") else {"available": False},
            "journal": self.journal_state(with_hash),
            "probes": {"version": version, "status": status, "login": login},
        }
        if self.bridge_initial is not None and index % 10 == 0:
            record["bridge_mapped"] = bridge_mapped(int(self.state["pid"]), Path(self.state["build"]["bridge"]))
            if not record["bridge_mapped"]:
                self.probe_failures.append({"t_utc": record["t_utc"], "kind": "bridge-unmapped"})
        self.samples.append(record)
        self.events.write(json.dumps({"kind": "sample", **record}, ensure_ascii=False) + "\n")
        self.events.flush()
        rehearsal = self.state.get("mode") == "rehearsal"
        for name, value in (("version", version), ("status", status), ("login", login)):
            if not value.get("ok") and not (rehearsal and name in ("status", "login")):
                self.probe_failures.append(
                    {"t_utc": record["t_utc"], "kind": name, "result": sanitize_probe(value)}
                )
        return record

    def wait_recovered(self, timeout: float = 180.0):
        deadline = time.time() + timeout
        last = None
        rehearsal = self.state.get("mode") == "rehearsal"
        while time.time() < deadline:
            last = probe(
                self.state["ports"]["http"],
                self.token,
                "get_version_info" if rehearsal else "get_login_info",
            )
            if last.get("ok"):
                return last, round(timeout - (deadline - time.time()), 2)
            time.sleep(1.0)
        return last, None

    def restart_core(self, index: int):
        before = self.journal_state(True)
        self.log_event("core_kill", index=index, journal=before)
        self.daemon_proc.kill()
        self.daemon_proc.wait(timeout=20)
        killed_at = time.time()
        proc, _ = self.start_daemon(index, int(self.state["pid"]) if self.state.get("pid") else None)
        recovered, recovered_after = self.wait_recovered()
        after = self.journal_state(True)
        prefix_ok = None
        if before.get("exists") and after.get("exists"):
            size = int(before.get("complete_bytes", 0))
            if size > 0:
                prefix_ok = sha256_prefix(Path(self.state["journal"]), size) == before.get("complete_sha256")
        record = {
            "index": index,
            "t_utc": utcnow(),
            "journal_before": before,
            "journal_after": after,
            "journal_prefix_preserved": prefix_ok,
            "recovered": bool(recovered and recovered.get("ok")),
            "recovery_seconds": round(time.time() - killed_at, 2),
            "probe": sanitize_probe(recovered),
        }
        self.restarts.append(record)
        self.log_event("core_restarted", **record)
        if not record["recovered"]:
            raise RuntimeError(f"core restart {index} did not recover within timeout: {recovered}")
        return record

    def summarize_process(self, name: str):
        values = [s[name] for s in self.samples if s.get(name, {}).get("available")]
        if not values:
            return {"available": False, "samples": 0}
        working = [v["working_set_bytes"] for v in values if v.get("working_set_bytes") is not None]
        handles = [v["handles"] for v in values if v.get("handles") is not None]
        private = [v["private_bytes"] for v in values if v.get("private_bytes") is not None]
        return {
            "available": True,
            "samples": len(values),
            "working_set_first": working[0] if working else None,
            "working_set_last": working[-1] if working else None,
            "working_set_max": max(working) if working else None,
            "private_first": private[0] if private else None,
            "private_last": private[-1] if private else None,
            "handles_first": handles[0] if handles else None,
            "handles_last": handles[-1] if handles else None,
            "handles_max": max(handles) if handles else None,
        }

    def hourly_summary(self, hour_index: int):
        recent = [s for s in self.samples if hour_index * 3600 <= s["elapsed_s"] < (hour_index + 1) * 3600]
        if not recent:
            return
        value = {
            "hour": hour_index,
            "t_utc": utcnow(),
            "samples": len(recent),
            "daemon_working_set_max": max(
                (s["daemon"].get("working_set_bytes") or 0) for s in recent if s["daemon"].get("available")
            ),
            "daemon_handles_max": max(
                (s["daemon"].get("handles") or 0) for s in recent if s["daemon"].get("available")
            ),
            "qq_working_set_max": max(
                (s["qq"].get("working_set_bytes") or 0) for s in recent if s["qq"].get("available")
            ),
            "probe_failures": sum(
                1
                for s in recent
                for p in s["probes"].values()
                if not p.get("ok")
            ),
            "journal_size": recent[-1]["journal"].get("size"),
        }
        (self.out / "hourly").mkdir(exist_ok=True)
        write_json(self.out / "hourly" / f"hour-{hour_index:02d}.json", value)
        print(json.dumps({"kind": "hourly", **value}, ensure_ascii=False), flush=True)

    def final_summary(self, duration_target: float):
        duration = time.time() - self.started
        probes = [p for s in self.samples for p in s["probes"].values()]
        latencies = [p["latency_ms"] for p in probes if "latency_ms" in p]
        probe_ok = sum(1 for p in probes if p.get("ok"))
        restart_ok = bool(self.restarts) and all(r["recovered"] and r.get("journal_prefix_preserved") is not False for r in self.restarts)
        journal_prefix = all(r.get("journal_prefix_preserved") is not False for r in self.restarts)
        bridge_retained = self.bridge_final is True
        qq_ok = False
        if self.state.get("pid"):
            identity = process_identity(int(self.state["pid"]))
            qq_ok = bool(identity and identity["responding"])
        daemon_summary = self.summarize_process("daemon")
        qq_summary = self.summarize_process("qq")
        bounded = True
        if daemon_summary.get("working_set_first") and daemon_summary.get("working_set_max"):
            bounded = bounded and daemon_summary["working_set_max"] < 1024 * 1024 * 1024
            bounded = bounded and (daemon_summary["working_set_last"] or 0) < daemon_summary["working_set_first"] * 3 + 256 * 1024 * 1024
        if daemon_summary.get("handles_first") and daemon_summary.get("handles_last"):
            bounded = bounded and daemon_summary["handles_last"] < daemon_summary["handles_first"] + 512
        rehearsal = self.state.get("mode") == "rehearsal"
        criteria = {
            "resident_duration": duration >= duration_target - 180,
            "probes_answered": probe_ok > 0 and len(self.probe_failures) == 0,
            "core_restarts_recovered": restart_ok,
            "journal_prefix_preserved": journal_prefix,
            "bridge_retained_after_stop": True if rehearsal else bridge_retained,
            "qq_responsive_after_stop": True if rehearsal else qq_ok,
            "process_growth_bounded": bounded,
        }
        overall = "pass" if all(criteria.values()) else "fail"
        if duration_target - duration > 180:
            overall = "incomplete"
        summary = {
            "schema": "caligo/n5-soak-summary/1",
            "mode": self.state.get("mode", "field"),
            "started_utc": self.start_utc,
            "finished_utc": utcnow(),
            "duration_seconds": round(duration, 2),
            "duration_target_seconds": round(duration_target, 2),
            "target_hours": self.state.get("duration_hours"),
            "sample_seconds": self.state.get("sample_seconds"),
            "samples": len(self.samples),
            "probe_count": len(probes),
            "probe_ok": probe_ok,
            "probe_failures": len(self.probe_failures),
            "probe_failure_samples": self.probe_failures[:20],
            "latency_ms": {
                "median": round(statistics.median(latencies), 2) if latencies else None,
                "p95": round(sorted(latencies)[int(len(latencies) * 0.95) - 1], 2) if latencies else None,
                "max": max(latencies) if latencies else None,
            },
            "daemon": daemon_summary,
            "qq": qq_summary,
            "restarts": self.restarts,
            "journal_initial": self.journal_initial,
            "journal_final": self.journal_final,
            "stop": self.stop_record,
            "bridge_initial": self.bridge_initial,
            "bridge_final": self.bridge_final,
            "criteria": criteria,
            "overall": overall,
            "fatal": self.fatal,
        }
        write_json(self.out / "soak-summary.json", summary)
        lines = [
            "# N5 soak summary",
            "",
            f"- mode: `{summary['mode']}`",
            f"- started: {summary['started_utc']}",
            f"- finished: {summary['finished_utc']}",
            f"- duration: {summary['duration_seconds']:.0f}s (target {summary['duration_target_seconds']:.0f}s)",
            f"- samples: {summary['samples']}, probes: {summary['probe_ok']}/{summary['probe_count']} ok",
            f"- daemon working set first/last/max: {daemon_summary.get('working_set_first')}/{daemon_summary.get('working_set_last')}/{daemon_summary.get('working_set_max')}",
            f"- daemon handles first/last/max: {daemon_summary.get('handles_first')}/{daemon_summary.get('handles_last')}/{daemon_summary.get('handles_max')}",
            f"- core restarts: {len(self.restarts)}",
            f"- bridge initial/final: {self.bridge_initial}/{self.bridge_final}",
            f"- criteria: {json.dumps(criteria, ensure_ascii=False)}",
            f"- overall: **{overall}**",
        ]
        if self.fatal:
            lines.append(f"- fatal: {self.fatal}")
        (self.out / "soak-summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
        return summary


def load_state(path: Path) -> dict:
    state = read_json(path)
    required = ["schema", "build", "pipe", "journal", "token_file", "ports", "account", "duration_hours", "sample_seconds"]
    for field in required:
        if field not in state:
            raise SystemExit(f"state is missing {field}")
    build = state["build"]
    for key in ("caligo_exe", "caligo_sha256", "bridge", "bridge_sha256"):
        if key not in build:
            raise SystemExit(f"state.build is missing {key}")
    if not Path(build["caligo_exe"]).exists() or not Path(build["bridge"]).exists():
        raise SystemExit("pinned release binaries are missing")
    if sha256_file(Path(build["caligo_exe"])).lower() != str(build["caligo_sha256"]).lower():
        raise SystemExit("pinned release EXE hash mismatch")
    if sha256_file(Path(build["bridge"])).lower() != str(build["bridge_sha256"]).lower():
        raise SystemExit("pinned bridge hash mismatch")
    token_path = Path(state["token_file"])
    if not token_path.exists():
        raise SystemExit("token file is missing")
    token = token_path.read_text(encoding="ascii").strip()
    if len(token) != 64 or any(c not in "0123456789abcdef" for c in token):
        raise SystemExit("token file is not 64 lowercase hex characters")
    return state


def rehearse_state(repo: Path, work: Path, hours: float, sample_seconds: int) -> dict:
    exe = repo / "target-harness/x86_64-pc-windows-msvc/release/caligo.exe"
    bridge = repo / "target-harness/x86_64-pc-windows-msvc/release/caligo_bridge.dll"
    if not exe.exists():
        exe = repo / "target-harness/release/caligo.exe"
        bridge = repo / "target-harness/release/caligo_bridge.dll"
    if not exe.exists() or not bridge.exists():
        raise SystemExit("release binaries are missing; run verify-release.ps1 first")
    work.mkdir(parents=True, exist_ok=True)
    token_file = work / "rehearsal.token"
    token_file.write_text(new_hex(32), encoding="ascii")
    return {
        "schema": "caligo/n5-soak-state/1",
        "mode": "rehearsal",
        "build": {
            "caligo_exe": str(exe),
            "caligo_sha256": sha256_file(exe),
            "bridge": str(bridge),
            "bridge_sha256": sha256_file(bridge),
        },
        "pipe": "caligo-n5-rehearsal-" + new_hex(4),
        "journal": str(work / "journal.jsonl"),
        "token_file": str(token_file),
        "ports": {"http": free_port(), "ws": free_port()},
        "account": 1694717255,
        "pid": 0,
        "duration_hours": hours,
        "sample_seconds": sample_seconds,
        "restart_at_minutes": [0.25],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="N5 resident soak and recovery harness")
    parser.add_argument("--state", help="field state JSON from preflight.ps1")
    parser.add_argument("--pack", help="field pack directory (defaults to state parent)")
    parser.add_argument("--out", help="output directory (defaults to pack)")
    parser.add_argument("--rehearse", action="store_true", help="run without QQ or bridge")
    parser.add_argument("--hours", type=float, help="override duration hours")
    parser.add_argument("--sample-seconds", type=int, help="override sample interval")
    parser.add_argument("--restart-at-min", type=float, nargs="*", help="override restart minutes")
    parser.add_argument("--repo", default=str(Path(__file__).resolve().parents[2]))
    args = parser.parse_args()

    repo = Path(args.repo).resolve()
    if args.rehearse:
        work = Path(args.out) if args.out else repo / "target-harness/n5-soak-rehearsal"
        state = rehearse_state(repo, work, args.hours or 0.02, args.sample_seconds or 5)
        if args.restart_at_min is not None:
            state["restart_at_minutes"] = list(args.restart_at_min)
        pack = work
    else:
        if not args.state:
            parser.error("--state is required unless --rehearse is used")
        state_path = Path(args.state).resolve()
        state = load_state(state_path)
        pack = Path(args.pack).resolve() if args.pack else state_path.parent
        if args.hours is not None:
            state["duration_hours"] = args.hours
        if args.sample_seconds is not None:
            state["sample_seconds"] = args.sample_seconds
        if args.restart_at_min is not None:
            state["restart_at_minutes"] = list(args.restart_at_min)
        authorization = pack / "authorization.private.json"
        if not authorization.exists():
            raise SystemExit(f"authorization is missing: {authorization}")
        authorization_data = read_json(authorization)
        if authorization_data.get("authorized") is not True:
            raise SystemExit("authorization.private.json is not authorized")
        if int(authorization_data.get("pid", 0)) != int(state.get("pid", 0)):
            raise SystemExit("authorization PID does not match state")
        if authorization_data.get("created_utc") != state.get("created"):
            raise SystemExit("authorization creation time does not match state")
        identity = process_identity(int(state["pid"]))
        if not identity or identity["created"] != state["created"] or not identity["responding"]:
            raise SystemExit("QQ target generation/health changed since preflight")
        if bridge_mapped(int(state["pid"]), Path(state["build"]["bridge"])):
            raise SystemExit("bridge is already mapped; use a fresh QQ generation")
        if Path(state["journal"]).exists() and Path(state["journal"]).stat().st_size > 0:
            raise SystemExit("journal already contains data; use a fresh pack")

    out = Path(args.out).resolve() if args.out else pack
    soak = Soak(state, pack, args)
    soak.token = Path(state["token_file"]).read_text(encoding="ascii").strip()
    summary = None
    try:
        soak.log_event("soak_start", mode=state.get("mode"), hours=state["duration_hours"], account=state.get("account"))
        soak.start_daemon(0, int(state["pid"]) if state.get("pid") else None)
        if not args.rehearse:
            result = subprocess.run(
                [
                    "C:\\windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    str(repo / "scripts/n5/attach.ps1"),
                    "-Pack",
                    str(pack),
                ],
                capture_output=True,
                text=True,
                timeout=120,
            )
            (pack / "attach.private.log").write_text(
                result.stdout + "\n" + result.stderr, encoding="utf-8"
            )
            if result.returncode != 0:
                raise RuntimeError(f"attach failed (exit {result.returncode}): {result.stderr[-2000:]}")
            soak.bridge_initial = bridge_mapped(int(state["pid"]), Path(state["build"]["bridge"]))
            if not soak.bridge_initial:
                raise RuntimeError("bridge is not mapped after attach")
            login = soak.wait_login()
            if not login or not login.get("ok"):
                raise RuntimeError(f"login was not confirmed after attach: {login}")
            soak.log_event("bridge_attached", bridge_mapped=soak.bridge_initial, login_ok=True)
        soak.journal_initial = soak.journal_state(True)

        target_seconds = float(state["duration_hours"]) * 3600
        restart_at = sorted(float(value) * 60 for value in state.get("restart_at_minutes", []))
        restart_index = 0
        sample_seconds = max(1.0, float(state["sample_seconds"]))
        next_sample = time.time()
        next_hour = 1
        index = 0
        while time.time() - soak.started < target_seconds:
            now = time.time()
            if now >= next_sample:
                soak.sample(index, with_hash=(index % 10 == 0))
                index += 1
                next_sample += sample_seconds
                if now - soak.started >= next_hour * 3600:
                    soak.hourly_summary(next_hour - 1)
                    next_hour += 1
            if restart_index < len(restart_at) and time.time() - soak.started >= restart_at[restart_index]:
                restart_index += 1
                soak.restart_core(restart_index)
                next_sample = time.time() + sample_seconds
            remaining = min(next_sample - time.time(), target_seconds - (time.time() - soak.started))
            if remaining > 0:
                time.sleep(min(remaining, 1.0))
        soak.log_event("soak_window_complete", samples=len(soak.samples))
    except Exception as error:  # noqa: BLE001 - evidence records the fatal boundary
        soak.fatal = f"{type(error).__name__}: {error}"
        soak.log_event("soak_fatal", error=soak.fatal)
    finally:
        try:
            stop_error = None
            if soak.daemon_proc and soak.daemon_proc.poll() is None:
                try:
                    soak.stop_daemon(force=True)
                except Exception as error:  # noqa: BLE001 - preserve the stop failure in evidence
                    stop_error = f"{type(error).__name__}: {error}"
                    soak.stop_record["stop_error"] = stop_error
                    if soak.daemon_proc and soak.daemon_proc.poll() is None:
                        soak.daemon_proc.kill()
            if state.get("pid"):
                soak.bridge_final = bridge_mapped(int(state["pid"]), Path(state["build"]["bridge"]))
                identity = process_identity(int(state["pid"]))
                soak.log_event(
                    "stop_boundary",
                    daemon_exited=soak.daemon_proc.poll() if soak.daemon_proc else None,
                    bridge_retained=soak.bridge_final,
                    qq_responding=bool(identity and identity["responding"]),
                    owner_stop="not_attempted_default_bridge",
                    stop_error=stop_error,
                )
            soak.journal_final = soak.journal_state(True)
            summary = soak.final_summary(target_seconds if "target_seconds" in locals() else 0)
            soak.log_event("soak_finished", overall=summary["overall"], criteria=summary["criteria"])
        except Exception as error:  # noqa: BLE001 - the harness must still close its evidence file
            soak.fatal = soak.fatal or f"finalization: {type(error).__name__}: {error}"
            print(json.dumps({"kind": "finalization_error", "error": soak.fatal}), flush=True)
        finally:
            soak.events.close()
    if soak.fatal:
        return 2
    return 0 if summary and summary.get("overall") == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
