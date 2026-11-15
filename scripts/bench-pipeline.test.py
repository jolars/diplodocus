"""Run with python scripts/bench-pipeline.test.py."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("pipeline_bench", Path(__file__).with_name("bench-pipeline.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class BenchmarkTests(unittest.TestCase):
    def test_mutations_preserve_source_and_restore_bytes_including_crlf(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "source.R"
            original = b"#' Old docs\r\nf <- function() 1\r\n"
            path.write_bytes(original)
            edit = bench.mutation(root, "source.R", "roxygen-gap", "Old docs", "New docs")
            self.assertEqual(path.read_bytes(), original)
            self.assertEqual(edit["changed"], b"#' New docs\r\nf <- function() 1\r\n")
            with self.assertRaises(ValueError):
                bench.mutation(root, "source.R", "signature", "missing", "replacement")

    def test_disposable_copy_retains_native_artifacts_and_does_not_write_through_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            (source / "extension.so").write_bytes(b"prepared extension")
            (source / "R").mkdir()
            (source / "R/api.R").symlink_to(source / "extension.so")
            (source / ".diplodocus").mkdir()
            (source / ".diplodocus/stale").touch()
            bench.copy_source(source, root / "copy")
            self.assertEqual((root / "copy/extension.so").read_bytes(), b"prepared extension")
            self.assertFalse((root / "copy/.diplodocus").exists())
            (root / "copy/R/api.R").write_bytes(b"changed")
            self.assertEqual((source / "extension.so").read_bytes(), b"prepared extension")

    def test_phase_summary_keeps_native_subphases_separate_from_process_time(self):
        summary = bench.phase_summary([{"kind": "startup", "events": [
            {"event": "phase-started", "id": 1, "phase": "native-process", "package": "demo", "target": None},
            {"event": "native-timing", "phase": "package-loading", "package": "demo", "target": None, "seconds": 2},
            {"event": "phase-finished", "id": 1, "seconds": 3},
        ]}])
        self.assertEqual({row["source"]: row["median_seconds"] for row in summary}, {"host": 3, "native": 2})
        self.assertEqual(bench.summarize([{"kind": "startup", "seconds": i} for i in range(1, 11)])
                         ["startup"], {"count": 10, "median_seconds": 5.5, "p95_seconds": 10})


if __name__ == "__main__":
    unittest.main()
