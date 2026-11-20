"""Run one of four Cargo integration shards, balanced by observed runtime."""

import argparse
import json
import subprocess

# Rounded durations in seconds from GitHub Actions run 44701059481.
# Short and newly added targets use the default weight below.
TIMINGS = {
    "api_examples": 384,
    "check_workspace": 40,
    "documenter": 194,
    "documenter_execution": 83,
    "execution_cache": 9,
    "execution_isolation": 14,
    "extract_command": 383,
    "extraction_watch": 206,
    "fragments": 20,
    "incremental_source_paths": 102,
    "julia_discovery": 38,
    "julia_extraction": 182,
    "julia_runtime": 95,
    "markdown_math": 12,
    "milestone_four": 10,
    "milestone_six": 12,
    "monorepo_example": 230,
    "native_adapter_contract": 135,
    "package_overviews": 82,
    "preview": 8,
    "python_class_presentation": 18,
    "python_public_api": 11,
    "python_surface": 67,
    "qmd_includes_execution": 7,
    "r_extraction": 74,
    "r_package_pages": 69,
    "r_rd_content": 67,
    "r_reference_presentation": 20,
    "r_roxygen": 90,
    "r_roxygen_extensions": 48,
    "rd_examples": 56,
    "rmarkdown": 63,
    "rust_extraction": 10,
    "sidebar_navigation": 23,
    "site_discovery": 17,
    "site_fragment_aliases": 38,
    "site_generation": 77,
    "site_images": 29,
    "site_page_metadata": 68,
    "site_redirects": 38,
    "snapshot_canonical": 17,
    "snapshot_metadata": 36,
    "snapshot_publication": 56,
    "snapshot_refresh": 607,
    "snapshot_schema": 18,
    "snapshot_validation": 150,
    "snapshots_storage": 63,
    "workspace_assembly": 31,
    "workspace_resolution": 101,
}


def integration_shards():
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]
        )
    )
    package = next(
        package for package in metadata["packages"] if package["name"] == "diplodocus"
    )
    targets = [
        target["name"] for target in package["targets"] if target["kind"] == ["test"]
    ]
    shards = [[] for _ in range(4)]
    totals = [0] * len(shards)
    for name in sorted(targets, key=lambda name: (-TIMINGS.get(name, 5), name)):
        index = min(range(len(shards)), key=totals.__getitem__)
        shards[index].append(name)
        totals[index] += TIMINGS.get(name, 5)
    return shards, totals


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite", choices=[f"integration-{i}" for i in range(1, 5)])
    parser.add_argument(
        "--list",
        action="store_true",
        help="Show the selected targets without running them.",
    )
    args = parser.parse_args()
    shards, totals = integration_shards()
    index = int(args.suite.rsplit("-", 1)[1]) - 1
    targets = shards[index]
    if not targets:
        parser.error(f"{args.suite} has no integration targets")
    print(
        f"{args.suite}: {len(targets)} targets, historical estimate {totals[index]}s",
        flush=True,
    )
    print(" ".join(targets), flush=True)
    if not args.list:
        command = ["cargo", "test", "--locked", "--no-fail-fast"]
        for target in targets:
            command.extend(["--test", target])
        return subprocess.run(command, check=False).returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
