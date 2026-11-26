#!/usr/bin/env python3
"""Measure the production preview loop using prebuilt pipeline_bench workers.

Run the entire comparison in one session on an idle machine. Copies and
validation happen outside measured intervals. The producer checkouts remain
available for other work.
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


def prepare(root, scenario, qualpal, qualpalr, qualpalr_patch=None, qualpalr_dll=None):
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
    elif scenario == "python":
        copy_source(ROOT / "examples/monorepo/python", root / "package")
        config["packages"] = [package("python", "python", "pyproject.toml", "python", "src/tinystats")]
        edits.extend([
            mutation(root, "package/src/tinystats/__init__.py", "api-prose",
                     "Compute the mean squared difference", MARKER + " Compute the mean squared difference"),
            mutation(root, "package/src/tinystats/__init__.py", "signature", "-> float:", "-> int:"),
        ])
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
        for name in ("DESCRIPTION", "NAMESPACE", ".Rbuildignore", "renv.lock", "NEWS.md", "NEWS.Rd", "NEWS", "LICENSE", "LICENCE"):
            if (qualpalr / name).is_file():
                shutil.copyfile(qualpalr / name, root / "package" / name)
        if qualpalr_patch is not None:
            applied = subprocess.run(
                ["git", "apply", str(qualpalr_patch)], cwd=root / "package",
                capture_output=True, text=True, check=False,
            )
            if applied.returncode:
                raise ValueError(f"Unable to prepare qualpalr input patch: {applied.stderr.strip()}")
        if qualpalr_dll is not None:
            destination = root / "package/src" / qualpalr_dll.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(qualpalr_dll, destination)
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
        polls = {}
        try:
            for line in self.process.stdout:
                event = json.loads(line)
                if event["event"] == "phase-started" and event["phase"] == "watch-poll":
                    polls[event["id"]] = event
                    continue
                if event["event"] == "phase-finished" and event["id"] in polls:
                    # Idle polling can cross a sample boundary. Queue its phase pair
                    # together so every recorded sample retains balanced telemetry.
                    event = {"event": "watch-poll", "events": [polls.pop(event["id"]), event]}
                self.lines.put((time.monotonic(), event))
        finally:
            self.lines.put((time.monotonic(), None))

    def record_event(self, event):
        if event["event"] == "watch-poll":
            self.events.extend(event["events"])
            return False
        self.events.append(event)
        return True

    def until(self, name, timeout, allow_failure=False):
        deadline = time.monotonic() + timeout
        while True:
            arrived, event = self.lines.get(timeout=max(0, deadline - time.monotonic()))
            if event is None:
                self.errors.seek(0)
                raise RuntimeError(self.errors.read() or "Benchmark worker exited")
            if not self.record_event(event):
                continue
            if event["event"] == "watching" and event["retained_previous"] and not allow_failure:
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
    path = root / ".diplodocus/documentation.sqlite"
    if not path.exists():
        return {"fingerprint": None, "tools": {}, "activities": [], "tool_observations": {}}
    with sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        fingerprint, = connection.execute("SELECT content_fingerprint FROM manifest").fetchone()
        tools = {}
        observations = {}
        activities = set()
        def collect(value):
            if isinstance(value, dict):
                if isinstance(value.get("activity"), dict):
                    activities.add(value["activity"].get("kind", ""))
                if isinstance(value.get("tools"), dict):
                    tools.update(value["tools"])
                    for identity, version in value["tools"].items():
                        observations.setdefault(identity, set()).add(version)
                for key, item in value.items():
                    if key in ("r_version", "julia_version", "runtime_version"):
                        tools[key] = item
                    collect(item)
            elif isinstance(value, list):
                for item in value:
                    collect(item)
        for content, in connection.execute("SELECT content FROM records"):
            collect(json.loads(content))
    return {"fingerprint": fingerprint, "tools": tools, "activities": sorted(activities),
            "tool_observations": {identity: sorted(versions)
                                  for identity, versions in sorted(observations.items())}}


def generation_edits(root):
    """Native generation consumes maintained R only; never patch its Rd outputs."""
    return [
        mutation(root, "docs/index.md", "authored", append=f"\n{MARKER}\n"),
        mutation(root, "package/R/qualpal.R", "api-prose",
                 "Generate qualitative color palettes", MARKER),
        mutation(root, "package/R/qualpal.R", "signature",
                 "refinement_starts = 5L", "refinement_starts = 5"),
        mutation(root, "package/R/qualpal.R", "exports", append=(
            "\n#' Pipeline benchmark export\n#' @export\n"
            "diplodocus_bench_export <- function() invisible(NULL)\n"
        )),
    ]


def assert_idle(worker, timeout=.6):
    """A generated checkout write must not queue another command by itself."""
    deadline = time.monotonic() + timeout
    while True:
        try:
            _, event = worker.lines.get(timeout=max(0, deadline - time.monotonic()))
        except queue.Empty:
            return
        if event is not None and not worker.record_event(event):
            continue
        raise RuntimeError(f"Preview did not remain idle after generation: {event}")


def generation_recovery(worker, root, args):
    """Exercise the real serve loop without including failure time in measurements."""
    source = root / "package/R/qualpal.R"
    original = source.read_bytes()
    before_site = inventory(root / "site")
    snapshot = root / ".diplodocus/documentation.sqlite"
    before_snapshot = digest(snapshot) if snapshot.exists() else None
    offset = len(worker.events)
    try:
        source.write_bytes(original + b"\n.diplodocus_benchmark_failure <- (\n")
        worker.until("error", args.timeout, allow_failure=True)
        worker.until("watching", args.timeout, allow_failure=True)
        if not worker.events[-1]["retained_previous"]:
            raise RuntimeError("Generation error did not retain the previous site")
        if before_site != inventory(root / "site") or before_snapshot != (digest(snapshot) if snapshot.exists() else None):
            raise RuntimeError("Generation error changed the published site or snapshot")
        assert_idle(worker)
        source.write_bytes(original)
        worker.until("rebuilt", args.timeout)
        worker.until("watching", args.timeout)
        if before_site != inventory(root / "site"):
            raise RuntimeError("Correcting the generation error did not restore the site")
        assert_idle(worker)
        return {"status": "ok", "retained_previous": True, "recovered": True,
                "events": worker.events[offset:]}
    finally:
        source.write_bytes(original)


def measure(binary, scenario, args, result):
    # Each startup uses a new workspace and fresh Diplodocus caches. OS and native
    # runtime caches are shared. One unreported run prepares those caches equally.
    for sample in range(-1, args.samples):
        with tempfile.TemporaryDirectory(prefix="diplodocus-bench-") as directory:
            root, config, edits = prepare(Path(directory), scenario, args.qualpal, args.qualpalr,
                                          args.qualpalr_patch, args.qualpalr_dll)
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
                generated = scenario == "qualpalr" and (
                    "r-documentation" in startup["activities"]
                    or any(event["event"] == "cache" and event.get("stage") == "r-generation"
                           for event in startup["events"])
                )
                if scenario == "qualpalr":
                    result["api_generation"] = "roxygen2" if generated else "maintained-rd"
                    if generated:
                        edits = generation_edits(root)
                    result["mutations"] = [{"kind": edit["kind"], "inputs": [
                        change["path"].relative_to(root).as_posix()
                        for change in [edit, *edit.get("companions", [])]
                    ]} for edit in edits]
                    result["comparison"] = (
                        "Equivalent initial maintained inputs; native API semantics, generated output, "
                        "and provenance can differ. Candidate generation saves edit R sources only; "
                        "maintained-Rd workers retain the roxygen-gap case."
                    )
                if generated:
                    assert_idle(worker)
                if sample >= 0:
                    result["samples"].append(startup)
                    print(f"  Startup {sample + 1}/{args.samples}: {startup['seconds']:.3f}s", flush=True)
                if sample == -1:
                    continue
                for edit in edits:
                    for save in range(args.saves):
                        offset = len(worker.events)
                        before = inventory(root / "site")
                        rd_before = inventory(root / "package/man") if scenario == "qualpalr" else None
                        namespace_before = digest(root / "package/NAMESPACE") if generated and edit["kind"] == "exports" else None
                        started = time.monotonic()
                        # Alternate the changed and original bytes. Every save is real.
                        for change in [edit, *edit.get("companions", [])]:
                            change["path"].write_bytes(change["changed"] if save % 2 == 0 else change["original"])
                        rebuilt = worker.until("rebuilt", args.timeout)
                        worker.until("watching", args.timeout)
                        record = {"kind": edit["kind"], "seconds": rebuilt - started,
                                  "events": worker.events[offset:], **snapshot_info(root)}
                        record["rendered_changed"] = before != inventory(root / "site")
                        if edit["kind"] == "roxygen-gap":
                            record["rd_unchanged"] = rd_before == inventory(root / "package/man")
                            record["known_gap"] = "Roxygen source edits do not regenerate maintained Rd."
                            if not record["rd_unchanged"] or record["rendered_changed"]:
                                raise RuntimeError("Roxygen baseline behavior changed; update the gap case")
                        elif not record["rendered_changed"]:
                            raise RuntimeError(f"{edit['kind']} edit did not change the rendered site")
                        if generated and edit["kind"] in ("api-prose", "signature", "exports"):
                            record["rd_changed"] = rd_before != inventory(root / "package/man")
                            if not record["rd_changed"]:
                                raise RuntimeError(f"{edit['kind']} source edit did not regenerate Rd")
                        if namespace_before is not None:
                            record["namespace_changed"] = namespace_before != digest(root / "package/NAMESPACE")
                            if not record["namespace_changed"]:
                                raise RuntimeError("Export source edit did not regenerate NAMESPACE")
                        if generated:
                            assert_idle(worker)
                        result["samples"].append(record)
                        print(f"  {edit['kind']} {save + 1}/{args.saves}: {record['seconds']:.3f}s", flush=True)
                    # Restore between mutation categories, and await its build if needed.
                    if args.saves % 2:
                        for change in [edit, *edit.get("companions", [])]:
                            change["path"].write_bytes(change["original"])
                        worker.until("rebuilt", args.timeout)
                        worker.until("watching", args.timeout)
                        if generated:
                            assert_idle(worker)
                if generated and sample == 0:
                    result["generation_recovery"] = generation_recovery(worker, root, args)
                if args.warm_restart:
                    expected_files = inventory(root / "site")
                    expected_snapshot = snapshot_info(root)["fingerprint"]
                    worker.close()
                    worker = Worker(binary, config, root / "site")
                    ready = worker.until("ready", args.timeout)
                    worker.until("watching", args.timeout)
                    record = {"kind": "warm-startup", "seconds": ready - worker.started,
                              "events": list(worker.events), **snapshot_info(root)}
                    if expected_files != inventory(root / "site") or expected_snapshot != record["fingerprint"]:
                        raise RuntimeError("Restarting with retained caches changed the completed site or snapshot")
                    if generated:
                        assert_idle(worker)
                    result["samples"].append(record)
                    print(f"  Warm restart {sample + 1}/{args.samples}: {record['seconds']:.3f}s", flush=True)
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


def cache_summary(samples):
    groups = {}
    for sample in samples:
        for event in sample["events"]:
            if event["event"] != "cache":
                continue
            key = (sample["kind"], event["stage"], event["owner"])
            counts = groups.setdefault(key, {"hits": 0, "misses": 0})
            counts["hits" if event["hit"] else "misses"] += 1
    return [{"kind": kind, "stage": stage, "owner": owner, **counts}
            for (kind, stage, owner), counts in sorted(groups.items())]


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
    for name in ("python", "R", "julia", "doxygen", "rustc"):
        try:
            result = subprocess.run([name, "--version"], capture_output=True, text=True, timeout=10)
            versions[name] = (result.stdout or result.stderr).strip() if result.returncode == 0 else "unavailable"
        except (OSError, subprocess.TimeoutExpired):
            versions[name] = "unavailable"
    return versions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", action="append", required=True, metavar="LABEL=EXECUTABLE")
    parser.add_argument("--scenario", action="append", choices=["authored", "python", "monorepo", "qualpal", "qualpalr"])
    parser.add_argument("--qualpal", type=Path, default=ROOT.parent / "qualpal")
    parser.add_argument("--qualpalr", type=Path, default=ROOT.parent / "qualpalr")
    parser.add_argument("--qualpalr-patch", type=Path,
                        help="apply a prepared input patch only in temporary qualpalr copies")
    parser.add_argument("--qualpalr-dll", type=Path,
                        help="copy this prepared shared library into temporary qualpalr src/ before measuring")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--saves", type=int, default=2)
    parser.add_argument("--warm-restart", action="store_true",
                        help="measure one restart per workspace with completed caches retained")
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.qualpalr_patch is not None:
        args.qualpalr_patch = args.qualpalr_patch.resolve(strict=True)
    if args.qualpalr_dll is not None:
        args.qualpalr_dll = args.qualpalr_dll.resolve(strict=True)
    if args.samples < 1 or args.saves < 1 or args.timeout <= 0:
        parser.error("samples, saves, and timeout must be positive")
    report = {"format_version": 1, "revision": revision(ROOT), "runtime_versions": command_versions(), "host": host_info(),
              "runtime_environment": {name: os.environ.get(name) for name in (
                  "LD_LIBRARY_PATH", "OMP_NUM_THREADS", "OMP_DYNAMIC", "OMP_PROC_BIND",
                  "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS")},
              "producer_revisions": {"qualpal": revision(args.qualpal), "qualpalr": revision(args.qualpalr)},
              "protocol": {"startup_samples": args.samples, "saves_per_startup_per_kind": args.saves,
                           "startup_cache": "fresh Diplodocus cache; shared warmed OS/runtime caches",
                           "warm_restart": args.warm_restart,
                           "timings": "inclusive host phases; native subphases overlap host durations"},
              "workers": {}}
    if args.qualpalr_patch is not None or args.qualpalr_dll is not None:
        report["input_preparation"] = {
            "scope": "Applied identically to temporary copies; producer checkout is unchanged.",
        }
        if args.qualpalr_patch is not None:
            report["input_preparation"].update(
                qualpalr_patch_path=str(args.qualpalr_patch),
                qualpalr_patch_sha256=digest(args.qualpalr_patch),
                qualpalr_patch=args.qualpalr_patch.read_text(),
            )
        if args.qualpalr_dll is not None:
            report["input_preparation"].update(
                qualpalr_dll_path=str(args.qualpalr_dll),
                qualpalr_dll_sha256=digest(args.qualpalr_dll),
            )
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
            result["cache_summary"] = cache_summary(result["samples"])
            results["scenarios"].append(result)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({k: v for k, v in result.items() if k in ("scenario", "status", "error", "summary")}))


if __name__ == "__main__":
    main()
