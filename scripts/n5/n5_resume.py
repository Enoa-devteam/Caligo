#!/usr/bin/env python3
"""Continue monitoring an already-running N5 field daemon.

This mode does not start, attach, restart or replace anything. It is used when
the short observation has passed and the operator decides to continue the same
resident session toward the long-duration gate. The original monitor is stopped
without sending a control Stop; this process takes over the same daemon, bridge
and journal, then performs the final control stop.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from analyze_journal import summarize as summarize_journal  # noqa: E402
from n5_soak import (  # noqa: E402
    bridge_mapped,
    process_identity,
    process_metrics,
    probe,
    read_json,
    sanitize_probe,
    utcnow,
    write_json,
)


def main() -> int:
    parser = argparse.ArgumentParser(description="Resume an N5 resident soak without reattaching")
    parser.add_argument("--state", required=True)
    parser.add_argument("--daemon", required=True)
    parser.add_argument("--hours", type=float, required=True)
    parser.add_argument("--sample-seconds", type=float, default=60)
    parser.add_argument("--out")
    args = parser.parse_args()

    state = read_json(Path(args.state).resolve())
    daemon = read_json(Path(args.daemon).resolve())
    pack = Path(args.state).resolve().parent
    out = Path(args.out).resolve() if args.out else pack
    out.mkdir(parents=True, exist_ok=True)

    exe = Path(state["build"]["caligo_exe"])
    bridge = Path(state["build"]["bridge"])
    token = Path(state["token_file"]).read_text(encoding="ascii").strip()
    if len(token) != 64:
        raise SystemExit("token file is invalid")
    pid = int(daemon["pid"])
    qq_pid = int(state["pid"])
    identity = process_identity(qq_pid)
    if not identity or identity["created"] != state["created"] or not identity["responding"]:
        raise SystemExit("QQ generation/health changed; refusing to resume")
    if not bridge_mapped(qq_pid, bridge):
        raise SystemExit("bridge is not mapped; refusing to resume")
    daemon_metrics = process_metrics(pid)
    if not daemon_metrics.get("available") or not daemon_metrics.get("alive"):
        raise SystemExit("daemon is not running; refusing to resume")
    check = probe(int(state["ports"]["http"]), token, "get_version_info")
    if not check.get("ok"):
        raise SystemExit(f"daemon HTTP is not answering; refusing to resume: {check}")

    started_utc = None
    events_log = pack / "soak-events.jsonl"
    if events_log.exists():
        for line in events_log.read_text(encoding="utf-8").splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            if record.get("kind") == "soak_start":
                started_utc = record.get("t_utc")
                break
    if not started_utc:
        started_utc = daemon.get("started_utc") or utcnow()
    started = dt.datetime.fromisoformat(started_utc.replace("Z", "+00:00"))
    uptime_before = max(0.0, (dt.datetime.now(dt.timezone.utc) - started).total_seconds())
    target_hours = args.hours
    target_seconds = target_hours * 3600
    start = time.time()
    events_path = out / "resume-events.jsonl"
    failures = []
    samples = []
    login_unavailable = 0

    print(json.dumps({"kind": "resume_start", "uptime_before_s": round(uptime_before, 2), "target_hours": target_hours}), flush=True)
    with events_path.open("a", encoding="utf-8") as events:
        next_sample = time.time()
        index = 0
        while time.time() - start < target_seconds:
            now = time.time()
            if now >= next_sample:
                version = probe(int(state["ports"]["http"]), token, "get_version_info")
                status = probe(int(state["ports"]["http"]), token, "get_status")
                login = probe(int(state["ports"]["http"]), token, "get_login_info")
                record = {
                    "t_utc": utcnow(),
                    "elapsed_s": round(time.time() - start, 2),
                    "daemon": process_metrics(pid),
                    "qq": process_metrics(qq_pid),
                    "journal_size": Path(state["journal"]).stat().st_size if Path(state["journal"]).exists() else None,
                    "probes": {"version": version, "status": status, "login": login},
                }
                if index % 10 == 0:
                    record["bridge_mapped"] = bridge_mapped(qq_pid, bridge)
                events.write(json.dumps({"kind": "sample", **record}, ensure_ascii=False) + "\n")
                events.flush()
                samples.append(record)
                login_locked = login.get("retcode") == 1502 and login.get("status") == "failed"
                if login_locked:
                    login_unavailable += 1
                elif not login.get("ok"):
                    failures.append({"t_utc": record["t_utc"], "kind": "login", "result": sanitize_probe(login)})
                for name, value in (("version", version), ("status", status)):
                    if not value.get("ok"):
                        failures.append({"t_utc": record["t_utc"], "kind": name, "result": sanitize_probe(value)})
                index += 1
                next_sample += max(1.0, args.sample_seconds)
                if index % 30 == 0:
                    print(json.dumps({"kind": "progress", "samples": index, "elapsed_s": record["elapsed_s"]}), flush=True)
            remaining = next_sample - time.time()
            if remaining > 0:
                time.sleep(min(remaining, 5.0))

        print(json.dumps({"kind": "resume_window_complete", "samples": len(samples)}), flush=True)
        stop_record = {}
        control = subprocess.run(
            [
                str(exe),
                "cli",
                "daemon-control",
                "--pipe",
                daemon["control_pipe"],
                "--auth",
                token,
                "stop",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
        stop_record.update({"control_exit": control.returncode, "stdout": control.stdout.strip(), "stderr": control.stderr.strip()})
        deadline = time.time() + 20
        while time.time() < deadline:
            if not process_metrics(pid).get("alive"):
                break
            time.sleep(0.2)
        stop_record["exited"] = not process_metrics(pid).get("alive")
        bridge_final = bridge_mapped(qq_pid, bridge)
        qq_final = process_identity(qq_pid)
        stop_record["bridge_retained"] = bridge_final
        stop_record["qq_responding"] = bool(qq_final and qq_final["responding"])

        journal = summarize_journal(Path(state["journal"]))
        duration = time.time() - start
        total_uptime = uptime_before + duration
        criteria = {
            "probes_answered": len(failures) == 0 and len(samples) > 0,
            "journal_intact": not journal["failures"] and not journal["event_seq_gaps"],
            "journal_no_duplicates": journal["duplicate_native_ids"] == 0,
            "bridge_retained_after_stop": bridge_final is True,
            "qq_responsive_after_stop": stop_record["qq_responding"] is True,
            "daemon_stopped_by_control": stop_record.get("control_exit") == 0 and stop_record.get("exited") is True,
            "continuous_uptime_24h": total_uptime >= 24 * 3600,
        }
        summary = {
            "schema": "caligo/n5-resume-summary/1",
            "started_utc": started_utc,
            "resume_started_utc": utcnow(),
            "duration_seconds": round(duration, 2),
            "uptime_before_seconds": round(uptime_before, 2),
            "total_continuous_seconds": round(total_uptime, 2),
            "total_continuous_hours": round(total_uptime / 3600, 3),
            "samples": len(samples),
            "probe_failures": len(failures),
            "probe_failure_samples": failures[:20],
            "login_unavailable": login_unavailable,
            "journal": journal,
            "stop": stop_record,
            "criteria": criteria,
            "overall": "pass" if all(criteria.values()) else "fail",
        }
        write_json(out / "resume-summary.json", summary)
        print(json.dumps({"kind": "resume_finished", "overall": summary["overall"], "criteria": criteria}), flush=True)
        return 0 if summary["overall"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
