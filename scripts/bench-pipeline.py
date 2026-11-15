#!/usr/bin/env python3
"""Measure the production preview loop using prebuilt pipeline_bench workers.

Run the entire comparison inside one compute bench reservation. Copies and
validation happen outside measured intervals. No producer checkout is modified.
"""

import argparse
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import queue
import shutil
import signal
import sqlite3
import statistics
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
MARKER = "Pipeline benchmark prose revision."


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inventory(root):
    return {
        path.relative_to(root).as_posix(): digest(path)
        for path in sorted(root.rglob("*"))
        if path.is_file() and not path.is_symlink()
    }


def copy_source(source, destination):
    shutil.copytree(
        source,
        destination,
        ignore=shutil.ignore_patterns(
            ".git", ".devenv*", ".direnv", ".diplodocus", "__pycache__",
            "node_modules", "target", "site", "build", "result", "result-*",
        ),
        # Dereference prepared input files, rather than writing through links to originals.
        symlinks=False,
    )


def revision(source):
    result = subprocess.run(
        ["git", "--no-optional-locks", "-C", str(source), "rev-parse", "HEAD"],
        capture_output=True, text=True, check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def mutation(root, path, kind, old=None, new=None, append=None):
    path = root / path
    original = path.read_bytes()
    if append is not None:
        changed = original + append.encode()
    else:
        old, new = old.encode(), new.encode()
        if old not in original:
            raise ValueError(f"Mutation input changed: {path.relative_to(root)} lacks {old!r}")
        changed = original.replace(old, new, 1)
    return {"path": path, "kind": kind, "original": original, "changed": changed}


def prepare(root, scenario, qualpal, qualpalr):
    """Explicit, focused cases. Missing real inputs are failures, never substitutions."""
    if scenario == "monorepo":
        copy_source(ROOT / "examples/monorepo", root / "workspace")
        root = root / "workspace"
        return root, root / "diplodocus.yaml", [
            mutation(root, "docs/overview.md", "authored", append=f"\n{MARKER}\n"),
            mutation(root, "r/man/mean_squared_error.Rd", "api-prose", "\\title{", f"\\title{{{MARKER} "),
            mutation(root, "python/src/tinystats/__init__.py", "signature", "-> float:", "-> int:"),
        ]

    root.mkdir(parents=True, exist_ok=True)
    (root / "docs").mkdir()
    (root / "docs/index.md").write_text("# Pipeline benchmark\n")
    config = {
        "project": {"name": "Pipeline benchmark"},
        "repositories": [{"id": "repo", "path": "."}],
        "content": [{"id": "guide", "owner": "project", "repository": "repo",
                     "path": "docs", "mount": "", "format": "gfm"}],
    }
    edits = [mutation(root, "docs/index.md", "authored", append=f"\n{MARKER}\n")]
    if scenario == "authored":
        pass
    elif scenario == "qualpal":
        copy_source(qualpal / "include", root / "package/include")
        (root / "package/Doxyfile").write_text(
            "PROJECT_NAME = Qualpal\nINPUT = include\nRECURSIVE = YES\n"
            "FILE_PATTERNS = *.h\nQUIET = YES\nEXTRACT_ALL = YES\n"
            "GENERATE_HTML = NO\nGENERATE_LATEX = NO\nGENERATE_XML = YES\n"
        )
        config["packages"] = [package("qualpal", "cpp", "Doxyfile", "doxygen", "Doxyfile")]
        edits.extend([
            mutation(root, "package/include/qualpal/qualpal.h", "api-prose",
                     "Set the maximum memory limit (in GB) for color difference matrix.", MARKER),
            mutation(root, "package/include/qualpal/qualpal.h", "signature",
                     "setMemoryLimit(double gb)", "setMemoryLimit(double gb = 1.0)"),
        ])
    elif scenario == "qualpalr":
        (root / "package").mkdir()
        # API-only profile: automatic vignette discovery is deliberately excluded.
        # Keep maintained help, native sources/artifacts, data, and runtime assets.
        for name in ("R", "man", "src", "inst", "data"):
            if (qualpalr / name).is_dir():
                copy_source(qualpalr / name, root / "package" / name)
        for name in ("DESCRIPTION", "NAMESPACE", "NEWS.md", "NEWS.Rd", "NEWS", "LICENSE", "LICENCE"):
            if (qualpalr / name).is_file():
                shutil.copyfile(qualpalr / name, root / "package" / name)
        (root / "package/BENCHMARK.md").write_text("# Qualpalr\nFocused API benchmark.\n")
        config["packages"] = [package("qualpalr", "r", "DESCRIPTION", "r", ".")]
        config["packages"][0]["overview"] = "BENCHMARK.md"
        edits.extend([
            mutation(root, "package/man/qualpal.Rd", "api-prose", "\\title{", f"\\title{{{MARKER} "),
            mutation(root, "package/R/qualpal.R", "signature",
                     "refinement_starts = 5L", "refinement_starts = 5"),
            mutation(root, "package/R/qualpal.R", "roxygen-gap",
                     "Generate qualitative color palettes", MARKER),
        ])
        # The current adapter requires maintained source and Rd usage to agree.
        edits[-2]["companions"] = [mutation(
            root, "package/man/qualpal.Rd", "signature",
            "refinement_starts = 5L", "refinement_starts = 5",
        )]
    else:
        raise ValueError(f"Unknown scenario: {scenario}")
    path = root / "diplodocus.yaml"
    path.write_text(json.dumps(config))  # JSON is also valid YAML.
    return root, path, edits


def package(name, ecosystem, metadata, extractor, target):
    return {"id": name, "name": name, "slug": name, "ecosystem": ecosystem,
            "repository": "repo", "path": "package", "metadata-path": metadata,
            "targets": [{"id": "api", "extractor": extractor, "path": target,
                         "role": "public-api"}]}


class Worker:
    def __init__(self, binary, config, output):
        self.events = []
        self.lines = queue.Queue()
        self.errors = tempfile.TemporaryFile(mode="w+")
        self.started = time.monotonic()
        self.process = subprocess.Popen(
            [str(binary), "--config", str(config), "--output", str(output)],
            stdout=subprocess.PIPE, stderr=self.errors, text=True,
        )
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        try:
            for line in self.process.stdout:
                self.lines.put((time.monotonic(), json.loads(line)))
        finally:
            self.lines.put((time.monotonic(), None))

    def until(self, name, timeout):
        deadline = time.monotonic() + timeout
        while True:
            arrived, event = self.lines.get(timeout=max(0, deadline - time.monotonic()))
            if event is None:
                self.errors.seek(0)
                raise RuntimeError(self.errors.read() or "Benchmark worker exited")
            self.events.append(event)
            if event["event"] == "watching" and event["retained_previous"]:
                raise RuntimeError(f"Rebuild failed; previous site retained: {self.events[-8:]}")
            if event["event"] == name:
                return arrived

    def close(self):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
                raise RuntimeError("Benchmark worker did not complete supervised shutdown")
        self.reader.join(timeout=5)
        self.process.stdout.close()
        self.errors.close()


def snapshot_info(root):
    with sqlite3.connect(root / ".diplodocus/documentation.sqlite") as connection:
        fingerprint, = connection.execute("SELECT content_fingerprint FROM manifest").fetchone()
        tools = {}
        def collect(value):
            if isinstance(value, dict):
                if isinstance(value.get("tools"), dict):
                    tools.update(value["tools"])
                for key, item in value.items():
                    if key in ("r_version", "julia_version", "runtime_version"):
                        tools[key] = item
                    collect(item)
            elif isinstance(value, list):
                for item in value:
                    collect(item)
        for content, in connection.execute("SELECT content FROM records"):
            collect(json.loads(content))
    return {"fingerprint": fingerprint, "tools": tools}


def measure(binary, scenario, args, result):
    # Each startup uses a new workspace and fresh Diplodocus caches. OS and native
    # runtime caches are shared. One unreported run prepares those caches equally.
    for sample in range(-1, args.samples):
        with tempfile.TemporaryDirectory(prefix="diplodocus-bench-") as directory:
            root, config, edits = prepare(Path(directory), scenario, args.qualpal, args.qualpalr)
            if sample == -1:
                inputs = inventory(root)
                result["inputs_sha256"] = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()
                result["inputs"] = inputs
            worker = Worker(binary, config, root / "site")
            try:
                ready = worker.until("ready", args.timeout)
                worker.until("watching", args.timeout)
                startup = {"kind": "startup", "seconds": ready - worker.started,
                           "events": list(worker.events), **snapshot_info(root)}
                if sample >= 0:
                    result["samples"].append(startup)
                    print(f"  Startup {sample + 1}/{args.samples}: {startup['seconds']:.3f}s", flush=True)
                if sample == -1:
                    continue
                for edit in edits:
                    for save in range(args.saves):
                        offset = len(worker.events)
                        before = inventory(root / "site")
                        rd_before = inventory(root / "package/man") if edit["kind"] == "roxygen-gap" else None
                        started = time.monotonic()
                        # Alternate the changed and original bytes. Every save is real.
                        for change in [edit, *edit.get("companions", [])]:
                            change["path"].write_bytes(change["changed"] if save % 2 == 0 else change["original"])
                        rebuilt = worker.until("rebuilt", args.timeout)
                        worker.until("watching", args.timeout)
                        record = {"kind": edit["kind"], "seconds": rebuilt - started,
                                  "events": worker.events[offset:], **snapshot_info(root)}
                        record["rendered_changed"] = before != inventory(root / "site")
                        if rd_before is not None:
                            record["rd_unchanged"] = rd_before == inventory(root / "package/man")
                            record["known_gap"] = "Roxygen source edits do not regenerate maintained Rd."
                            if not record["rd_unchanged"] or record["rendered_changed"]:
                                raise RuntimeError("Roxygen baseline behavior changed; update the gap case")
                        elif not record["rendered_changed"]:
                            raise RuntimeError(f"{edit['kind']} edit did not change the rendered site")
                        result["samples"].append(record)
                    # Restore between mutation categories, and await its build if needed.
                    if args.saves % 2:
                        for change in [edit, *edit.get("companions", [])]:
                            change["path"].write_bytes(change["original"])
                        worker.until("rebuilt", args.timeout)
                        worker.until("watching", args.timeout)
            finally:
                worker.close()
    result["summary"] = summarize(result["samples"])
    return result


def summarize(samples):
    groups = {}
    for sample in samples:
        groups.setdefault(sample["kind"], []).append(sample["seconds"])
    return {kind: {"count": len(values), "median_seconds": statistics.median(values),
                   "p95_seconds": sorted(values)[math.ceil(len(values) * .95) - 1]}
            for kind, values in groups.items()}


def phase_summary(samples):
    """Aggregate repeated processes within a sample, then summarize samples.

    Host and helper measurements have separate keys because their durations
    overlap. No sum of these keys is an end-to-end duration.
    """
    groups = {}
    for sample in samples:
        active, totals = {}, {}
        for event in sample["events"]:
            if event["event"] == "phase-started":
                active[event["id"]] = event
            elif event["event"] == "phase-finished":
                start = active.pop(event["id"])
                key = ("host", start["phase"], start["package"], start["target"])
                totals[key] = totals.get(key, 0) + event["seconds"]
            elif event["event"] == "native-timing":
                key = ("native", event["phase"], event["package"], event["target"])
                totals[key] = totals.get(key, 0) + event["seconds"]
        if active:
            raise ValueError("Unbalanced measurement phases")
        for (source, phase, package_id, target), seconds in totals.items():
            key = (sample["kind"], source, phase, package_id, target)
            groups.setdefault(key, []).append(seconds)
    return [{"kind": kind, "source": source, "phase": phase, "package": package_id,
             "target": target, "count": len(values), "median_seconds": statistics.median(values),
             "p95_seconds": sorted(values)[math.ceil(len(values) * .95) - 1]}
            for (kind, source, phase, package_id, target), values in groups.items()]


def host_info():
    cpuinfo = Path("/proc/cpuinfo")
    model = next((line.split(":", 1)[1].strip()
                  for line in cpuinfo.read_text().splitlines() if line.startswith("model name")), "") if cpuinfo.exists() else platform.processor()
    return {"system": platform.system(), "kernel": platform.release(),
            "architecture": platform.machine(), "cpu": model, "logical_cpus": os.cpu_count()}


def command_versions():
    versions = {}
    for name in ("python", "R", "julia", "doxygen"):
        try:
            result = subprocess.run([name, "--version"], capture_output=True, text=True, timeout=10)
            versions[name] = (result.stdout or result.stderr).strip() if result.returncode == 0 else "unavailable"
        except (OSError, subprocess.TimeoutExpired):
            versions[name] = "unavailable"
    return versions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", action="append", required=True, metavar="LABEL=EXECUTABLE")
    parser.add_argument("--scenario", action="append", choices=["authored", "monorepo", "qualpal", "qualpalr"])
    parser.add_argument("--qualpal", type=Path, default=ROOT.parent / "qualpal")
    parser.add_argument("--qualpalr", type=Path, default=ROOT.parent / "qualpalr")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--saves", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.samples < 1 or args.saves < 1 or args.timeout <= 0:
        parser.error("samples, saves, and timeout must be positive")
    report = {"format_version": 1, "revision": revision(ROOT), "runtime_versions": command_versions(), "host": host_info(),
              "producer_revisions": {"qualpal": revision(args.qualpal), "qualpalr": revision(args.qualpalr)},
              "protocol": {"startup_samples": args.samples, "saves_per_startup_per_kind": args.saves,
                           "startup_cache": "fresh Diplodocus cache; shared warmed OS/runtime caches",
                           "timings": "inclusive host phases; native subphases overlap host durations"},
              "workers": {}}
    for declaration in args.worker:
        label, binary = declaration.split("=", 1)
        binary = Path(binary).resolve(strict=True)
        if label in report["workers"]:
            parser.error(f"Duplicate worker label: {label}")
        results = {"executable_sha256": digest(binary), "scenarios": []}
        report["workers"][label] = results
        for scenario in args.scenario or ["monorepo", "qualpal", "qualpalr"]:
            print(f"Measuring {label}: {scenario}", flush=True)
            result = {"scenario": scenario, "status": "ok", "samples": []}
            try:
                measure(binary, scenario, args, result)
            except (OSError, ValueError, RuntimeError, queue.Empty) as error:
                result.update(status="blocked", error=str(error) or "Worker timed out")
            result["summary"] = summarize(result["samples"])
            result["phase_summary"] = phase_summary(result["samples"])
            results["scenarios"].append(result)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({k: v for k, v in result.items() if k in ("scenario", "status", "error", "summary")}))


if __name__ == "__main__":
    main()
