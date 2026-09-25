#!/usr/bin/env python3
"""Measure runner-wide no-completion windows in nextest JUnit files (#822).

Each <testcase> carries `timestamp` (its start) and `time` (its duration), so the
completion instants are timestamp + time. A "window" is a gap between consecutive
completions longer than --min-gap seconds. For each window, the script reports
the tests that were in flight across the whole window (started before it opened
and ended after it closed), and how many of those are derived-access tests.

Two output shapes:

- Per-file (default): one line per input FILE, plus its windows.
- Aggregate (--summary): group FILEs by a label passed alongside each one — the
  CI leg a file came from — and print one row per label: shard runs, runs with
  a window over 30s/60s (count and %), wall-time median, the median of each
  run's derived-access test-time median, and the largest window. In this mode
  each positional argument is `LABEL=FILE` rather than a bare path, so the
  same file layout works whether the caller groups by OS, shard count, or
  anything else meaningful to it.

Usage:
  junit_gaps.py [--min-gap 30] [--json] FILE...
  junit_gaps.py --summary [--min-gap 30] [--json] LABEL=FILE...
"""
import argparse
import json
import sys
from datetime import datetime

try:  # Inputs are this repo's own CI artifacts; prefer the hardened parser when available.
    import defusedxml.ElementTree as ET
except ImportError:  # pragma: no cover
    import xml.etree.ElementTree as ET

# Fixed headline thresholds for --summary, matching the #822 baseline (11% of
# 305 Windows shard runs had a window over 30s, 4% over 60s). --min-gap still
# controls the per-file window listing below; it is not applied to these two
# columns, so the report stays comparable across runs regardless of how a
# caller tunes --min-gap for the detailed view.
SUMMARY_GAP_THRESHOLDS_S = (30.0, 60.0)


def parse(path):
    cases = []
    for tc in ET.parse(path).getroot().iter("testcase"):
        ts = tc.get("timestamp")
        dur = tc.get("time")
        if not ts or dur is None:
            continue
        start = datetime.fromisoformat(ts).timestamp()
        d = float(dur)
        cases.append((start, start + d, d, tc.get("classname", ""), tc.get("name", "")))
    return cases


def analyze(path, min_gap):
    cases = parse(path)
    if not cases:
        return {"file": path, "tests": 0}
    ends = sorted(c[1] for c in cases)
    t0 = min(c[0] for c in cases)
    windows = []
    for a, b in zip(ends, ends[1:]):
        if b - a > min_gap:
            inflight = [c for c in cases if c[0] < a and c[1] > b]
            windows.append({
                "opens_s": round(a - t0, 1),
                "gap_s": round(b - a, 1),
                "in_flight": len(inflight),
                "derived_in_flight": sum("derived_access" in c[4] for c in inflight),
                "tests": sorted(f"{c[3]}::{c[4]} ({c[2]:.1f}s)" for c in inflight),
            })
    derived = sorted(c[2] for c in cases if "derived_access" in c[4])
    return {
        "file": path,
        "tests": len(cases),
        "wall_s": round(max(c[1] for c in cases) - t0, 1),
        "max_gap_s": round(max((b - a for a, b in zip(ends, ends[1:])), default=0.0), 1),
        "windows": windows,
        "derived_tests": len(derived),
        "derived_p50_s": round(derived[len(derived) // 2], 3) if derived else None,
        "derived_max_s": round(derived[-1], 3) if derived else None,
    }


def median(values):
    """The same simple (lower, unaveraged) median convention `analyze` uses."""
    if not values:
        return None
    s = sorted(values)
    return s[len(s) // 2]


def summarize_group(label, files):
    """Aggregate one CI leg's runs into the --summary row for `label`."""
    results = [analyze(f, min(SUMMARY_GAP_THRESHOLDS_S)) for f in files]
    valid = [r for r in results if r["tests"]]
    runs = len(valid)
    low, high = SUMMARY_GAP_THRESHOLDS_S
    over_low = sum(1 for r in valid if r["max_gap_s"] > low)
    over_high = sum(1 for r in valid if r["max_gap_s"] > high)
    derived_medians = [r["derived_p50_s"] for r in valid if r["derived_p50_s"] is not None]
    return {
        "label": label,
        "runs": runs,
        "unreadable": len(results) - runs,
        f"over_{low:g}s": over_low,
        f"over_{low:g}s_pct": round(100 * over_low / runs, 1) if runs else None,
        f"over_{high:g}s": over_high,
        f"over_{high:g}s_pct": round(100 * over_high / runs, 1) if runs else None,
        "wall_median_s": median([r["wall_s"] for r in valid]),
        "derived_median_of_medians_s": median(derived_medians),
        "largest_window_s": max((r["max_gap_s"] for r in valid), default=0.0),
    }


def print_summary_table(summaries):
    low, high = SUMMARY_GAP_THRESHOLDS_S
    headers = [
        "Leg",
        "Shard runs",
        f"Windows >{low:g}s",
        f"Windows >{high:g}s",
        "Wall median (s)",
        "Derived p50 median (s)",
        "Largest window (s)",
    ]
    print("| " + " | ".join(headers) + " |")
    print("| " + " | ".join(["---"] * len(headers)) + " |")
    for s in summaries:
        if not s["runs"]:
            print(f"| {s['label']} | 0 | n/a | n/a | n/a | n/a | n/a |")
            continue
        over_low = f"{s[f'over_{low:g}s']} ({s[f'over_{low:g}s_pct']}%)"
        over_high = f"{s[f'over_{high:g}s']} ({s[f'over_{high:g}s_pct']}%)"
        wall = f"{s['wall_median_s']:.1f}"
        derived = (
            f"{s['derived_median_of_medians_s']:.3f}"
            if s["derived_median_of_medians_s"] is not None
            else "n/a"
        )
        largest = f"{s['largest_window_s']:.1f}"
        print(
            f"| {s['label']} | {s['runs']} | {over_low} | {over_high} | "
            f"{wall} | {derived} | {largest} |"
        )


def parse_summary_args(entries):
    """Group `LABEL=FILE` entries into an ordered {label: [files]} mapping."""
    groups = {}
    order = []
    for entry in entries:
        label, sep, path = entry.partition("=")
        if not sep:
            raise SystemExit(
                f"--summary requires LABEL=FILE arguments, got: {entry!r}"
            )
        if label not in groups:
            groups[label] = []
            order.append(label)
        groups[label].append(path)
    return [(label, groups[label]) for label in order]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--min-gap", type=float, default=30.0)
    ap.add_argument("--json", action="store_true")
    ap.add_argument(
        "--summary",
        action="store_true",
        help="aggregate mode: each FILE argument is LABEL=FILE (e.g. the CI OS leg)",
    )
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()

    if args.summary:
        groups = parse_summary_args(args.files)
        summaries = [summarize_group(label, files) for label, files in groups]
        if args.json:
            json.dump(summaries, sys.stdout, indent=2)
            print()
        else:
            print_summary_table(summaries)
        return

    results = [analyze(f, args.min_gap) for f in args.files]
    if args.json:
        json.dump(results, sys.stdout, indent=2)
        print()
        return
    for r in results:
        if not r["tests"]:
            print(f"{r['file']}: no timestamped testcases")
            continue
        print(f"{r['file']}: tests={r['tests']} wall={r['wall_s']}s max_gap={r['max_gap_s']}s "
              f"windows>{args.min_gap:g}s={len(r['windows'])} derived_p50={r['derived_p50_s']}s "
              f"derived_max={r['derived_max_s']}s")
        for w in r["windows"]:
            print(f"  window at +{w['opens_s']}s: {w['gap_s']}s, in flight {w['in_flight']} "
                  f"({w['derived_in_flight']} derived-access)")


if __name__ == "__main__":
    main()
