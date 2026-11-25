#!/usr/bin/env python3
"""Content-free journal accounting for the N5 soak.

The journal is a framed binary log: `CLGJ` + version + payload length + CRC32 +
UTF-8 JSON. This tool reports only counts and sequence gaps; it never prints
message text, sender UINs, group codes or native message IDs.
"""

from __future__ import annotations

import argparse
import collections
import json
import struct
import zlib
from pathlib import Path

MAGIC = b"CLGJ"


def parse(path: Path):
    data = path.read_bytes()
    offset = 0
    records = []
    failures = []
    version = None
    while offset < len(data):
        if offset + 16 > len(data):
            failures.append({"offset": offset, "error": "truncated header"})
            break
        if data[offset : offset + 4] != MAGIC:
            failures.append({"offset": offset, "error": "bad magic"})
            break
        version, length, expected_crc = struct.unpack("<III", data[offset + 4 : offset + 16])
        body_start = offset + 16
        body_end = body_start + length
        if body_end > len(data):
            failures.append({"offset": offset, "error": "truncated payload"})
            break
        body = data[body_start:body_end]
        actual_crc = zlib.crc32(body) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            failures.append({"offset": offset, "error": "crc mismatch"})
            break
        try:
            records.append(json.loads(body))
        except json.JSONDecodeError as error:
            failures.append({"offset": offset, "error": f"json: {error}"})
            break
        offset = body_end
    return {
        "bytes": len(data),
        "records": records,
        "consumed_bytes": offset,
        "failures": failures,
        "version": version if records else None,
    }


def summarize(path: Path):
    parsed = parse(path)
    records = parsed["records"]
    kinds = collections.Counter(record.get("t") for record in records)
    events = [record.get("d", {}) for record in records if record.get("t") == "Event"]
    event_seqs = [event.get("event_seq") for event in events if isinstance(event.get("event_seq"), int)]
    native_ids = [event.get("native_id") for event in events if event.get("native_id")]
    counts = collections.Counter(native_ids)
    duplicates = {key: value for key, value in counts.items() if value > 1}
    gaps = []
    if event_seqs:
        low, high = min(event_seqs), max(event_seqs)
        present = set(event_seqs)
        gaps = [value for value in range(low, high + 1) if value not in present][:50]
    return {
        "journal": str(path),
        "bytes": parsed["bytes"],
        "consumed_bytes": parsed["consumed_bytes"],
        "frame_version": parsed["version"],
        "records": len(records),
        "kinds": dict(kinds),
        "events": len(events),
        "event_seq_min": min(event_seqs) if event_seqs else None,
        "event_seq_max": max(event_seqs) if event_seqs else None,
        "event_seq_gaps": gaps,
        "unique_native_ids": len(counts),
        "duplicate_native_ids": sum(value - 1 for value in duplicates.values()),
        "duplicate_id_samples": len(duplicates),
        "session_generations": sorted({event.get("session_generation") for event in events}),
        "directions": dict(collections.Counter(event.get("direction") for event in events)),
        "failures": parsed["failures"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="N5 journal accounting without message content")
    parser.add_argument("journal")
    parser.add_argument("--out")
    args = parser.parse_args()
    result = summarize(Path(args.journal))
    text = json.dumps(result, ensure_ascii=False, indent=2)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    print(text)
    return 1 if result["failures"] or result["event_seq_gaps"] or result["duplicate_native_ids"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
