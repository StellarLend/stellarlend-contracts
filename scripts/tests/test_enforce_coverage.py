#!/usr/bin/env python3
"""Unit tests for enforce_coverage.py."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from enforce_coverage import (
    CoverageError,
    _collect_packages,
    _parse_ratio,
    _parse_threshold,
    _resolve_coverage_path,
    get_threshold,
    load_thresholds,
)


def make_cobertura(packages, overall_line_rate=None):
    """Build a Cobertura XML tree for testing.

    *packages* is a list of ``(name, line_rate)`` tuples.
    """
    pkg_elems = []
    for name, lr in packages:
        pkg = ET.SubElement(ET.Element("package"), {"name": name, "line-rate": str(lr)})
        pkg_elems.append(pkg)

    if overall_line_rate is not None:
        root = ET.Element("coverage", {"line-rate": str(overall_line_rate)})
    else:
        root = ET.Element("coverage", {"line-rate": "1"})
    root.extend(pkg_elems)
    return root


class TestLoadThresholds(unittest.TestCase):
    def test_missing_file_returns_defaults(self):
        result = load_thresholds("/nonexistent/path.json")
        self.assertEqual(result["flat_threshold"], 95.0)
        self.assertEqual(result["per_crate"], {})

    def test_loads_valid_json(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"flat_threshold": 90.0, "per_crate": {"foo/src": 80.0}}, f)
            f.flush()
            result = load_thresholds(f.name)
            self.assertEqual(result["flat_threshold"], 90.0)
            self.assertEqual(result["per_crate"]["foo/src"], 80.0)
        os.unlink(f.name)

    def test_partial_json_uses_defaults(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"per_crate": {"bar/src": 85.0}}, f)
            f.flush()
            result = load_thresholds(f.name)
            self.assertEqual(result["flat_threshold"], 95.0)
            self.assertEqual(result["per_crate"]["bar/src"], 85.0)
        os.unlink(f.name)

    def test_none_path_returns_defaults(self):
        result = load_thresholds(None)
        self.assertEqual(result["flat_threshold"], 95.0)

    def test_malformed_json_raises(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("{not json")
            f.flush()
            with self.assertRaises(CoverageError):
                load_thresholds(f.name)
        os.unlink(f.name)

    def test_non_object_json_raises(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump([1, 2, 3], f)
            f.flush()
            with self.assertRaises(CoverageError):
                load_thresholds(f.name)
        os.unlink(f.name)

    def test_out_of_range_flat_threshold_raises(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"flat_threshold": 150.0}, f)
            f.flush()
            with self.assertRaises(CoverageError):
                load_thresholds(f.name)
        os.unlink(f.name)

    def test_negative_per_crate_threshold_raises(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"per_crate": {"foo/src": -1.0}}, f)
            f.flush()
            with self.assertRaises(CoverageError):
                load_thresholds(f.name)
        os.unlink(f.name)

    def test_nonnumeric_threshold_raises(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"flat_threshold": "not-a-number"}, f)
            f.flush()
            with self.assertRaises(CoverageError):
                load_thresholds(f.name)
        os.unlink(f.name)

    def test_per_crate_not_dict_raises(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"per_crate": [1, 2]}, f)
            f.flush()
            with self.assertRaises(CoverageError):
                load_thresholds(f.name)
        os.unlink(f.name)


class TestThresholdConfig(unittest.TestCase):
    def test_config_omits_excluded_legacy_crates(self):
        config_path = Path(__file__).resolve().parents[1] / "coverage_thresholds.json"
        with config_path.open() handle:
            thresholds = json.load(handle)

        self.assertEqual(thresholds["flat_threshold"], 70.0)
        self.assertNotIn("contracts/vesting/src", thresholds["per_crate"])
        self.assertNotIn("contracts/hello-world/src", thresholds["per_crate"])



class TestGetThreshold(unittest.TestCase):
    def setUp(self):
        self.thresholds = {
            "flat_threshold": 95.0,
            "per_crate": {
                "contracts/lending/src": 90.0,
                "contracts/common/src": 85.0,
            },
        }

    def test_crate_specific_threshold(self):
        self.assertEqual(get_threshold("contracts/lending/src", self.thresholds), 90.0)

    def test_falls_back_to_flat(self):
        self.assertEqual(get_threshold("contracts/unknown/src", self.thresholds), 95.0)

    def test_empty_per_crate_falls_back_to_flat(self):
        thresh = {"flat_threshold": 80.0, "per_crate": {}}
        self.assertEqual(get_threshold("anything", thresh), 80.0)


class TestResolveCoveragePath(unittest.TestCase):
    def test_existing_file_returns_as_is(self):
        with tempfile.NamedTemporaryFile(suffix=".xml", delete=False) as f:
            f.write(b<xml/>")
            f.flush()
            result = _resolve_coverage_path(f.name)
            self.assertEqual(result, f.name)
        os.unlink(f.name)

    def test_nonexistent_file_returns_input(self):
        result = _resolve_coverage_path("/definitely/does/not/exist.xml")
        self.assertEqual(result, "/definitely/does/not/exist.xml")



class TestParseHelpers(unittest.TestCase):
    def test_parse_ratio_accepts_boundaries(self):
        self.assertEqual(_parse_ratio("0", "rate"), 0.0)
        self.assertEqual(_parse_ratio("1", "rate"), 1.0)
        self.assertEqual(_parse_ratio(0.5, "rate"), 0.5)

    def test_parse_ratio_rejects_out_of_range(self):
        for bad in ("-0.01", "1.01", "NaN", "inf", "abc"):
            with self.assertRaises(CoverageError):
                _parse_ratio(bad, "rate")

    def test_parse_threshold_accepts_boundaries(self):
        self.assertEqual(_parse_threshold("0", "t"), 0.0)
        self.assertEqual(_parse_threshold("100", "t"), 100.0)

    def test_parse_threshold_rejects_out_of_range(self):
        for bad in ("-1", "100.1", "NaN", "inf", "x"):
            with self.assertRaises(CoverageError):
                _parse_threshold(bad, "t")


class TestCollectPackages(unittest.TestCase):
    def test_deduplicates_to_worst_coverage(self):
        root = make_cobertura(
            [
                ("contracts/common/src", 0.99),
                ("contracts/common/src", 0.50),
                ("contracts/common/src", 0.95),
            ],
            overall_line_rate=0.99,
        )
        packages = _collect_packages(root)
        self.assertEqual(packages["contracts/common/src"], 50.0)

    def test_missing_line_rate_reported_as_none(self):
        root = make_cobertura(
            [
                ("contracts/common/src", 0.99),
            ],
            overall_line_rate=0.99,
        )
        pkg = root.find("./package")
        del pkg.attrib["line-rate"]
        packages = _collect_packages(root)
        self.assertIsInstance(packages["contracts/common/src"], type(None))

    def test_invalid_line_rate_raises(self):
        root = make_cobertura([("contracts/common/src", 1.2)], overall_line_rate=0.99)
        with self.assertRaises(CoverageError):
            _collect_packages(root)

    def test_non_numeric_line_rate_raises(self):
        root = make_cobertura([("contracts/common/src", "abc")], overall_line_rate=0.99)
        with self.assertRaises(CoverageError):
            _collect_packages(root)



class TestMainIntegration(unittest.TestCase):
    def setUp(self):
        self.script_dir = os.path.join(os.path.dirname(__file__), "..")
        self.enforce = os.path.join(self.script_dir, "enforce_coverage.py")

    def _run(self, coverage_xml, extra_args=None):
        """Run enforce_coverage.py with a temp cobertura.xml, return (exit_code, output)."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".xml", delete=False) as f:
            f.write(coverage_xml)
            f.flush()
            xml_path = f.name

        cmd = [sys.executable, self.enforce, xml_path, "--thresholds-json", "/dev/null"]
        if extra_args:
            cmd.extend(extra_args)
        proc = subprocess.run(cmd, capture_output=True, text=True)
        os.unlink(xml_path)
        return proc.returncode, proc.stdout, proc.stderr

    def test_all_packages_pass(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.96">
  <packages>
    <package name="contracts/lending/src" line-rate="0.97"/>
    <package name="contracts/common/src" line-rate="0.96"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text)
        self.assertEqual(rc, 0, f"Expected pass, got exit {rc}\nstdout:{out}\nstderr:{err}")

    def test_one_package_fails(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.90">
  <packages>
    <package name="contracts/lending/src" line-rate="0.97"/>
    <package name="contracts/common/src" line-rate="0.80"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text)
        self.assertNotEqual(rc, 0, "Expected failure")
        self.assertIn("contracts/common/src", out)

    def test_threshold_override_raises_bar(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.94">
  <packages>
    <package name="contracts/lending/src" line-rate="0.94"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text, ["--threshold", "99.0"])
        self.assertNotEqual(rc, 0, "Expected failure with --threshold 99")

    def test_no_packages_fails(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0">
  <packages>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text)
        self.assertNotEqual(rc, 0, "Expected failure with no packages")
        self.assertIn("No <package>", out)

    def test_missing_line_rate_skips_package(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="1">
  <packages>
    <package name="contracts/lending/src"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text)
        self.assertEqual(rc, 0, "Expected pass when one package has no line-rate")

    def test_invalid_line_rate_fails_closed(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.99">
  <packages>
    <package name="contracts/lending/src" line-rate="1.5"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text)
        self.assertNotEqual(rc, 0, "Expected failure for out-of-range line-rate")
        self.assertIn("Error", out)

    def test_duplicate_crate_uses_worst_coverage(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.99">
  <packages>
    <package name="contracts/common/src" line-rate="0.99"/>
    <package name="contracts/common/src" line-rate="0.50"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text)
        self.assertNotEqual(rc, 0, "Duplicate crate must not mask failing coverage")
        self.assertIn("contracts/common/src", out)

    def test_overall_only_still_enforces_overall(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.50">
  <packages>
    <package name="contracts/common/src" line-rate="0.99"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text, ["--overall-only"])
        self.assertNotEqual(rc, 0, "Overall line rate below threshold must fail")
        self.assertIn("(overall)", out)

    def test_overall_only_passes_when_overall_mets_bar(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.99">
  <packages>
    <package name="contracts/common/src" line-rate="0.10"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text, ["--overall-only"])
        self.assertEqual(rc, 0, f"Expected pass, got {rc}\nstdout:{out}\nstderr:{err}")

    def test_malformed_xml_fails_closed(self):
        rc, out, err = self._run("<coverage><definitely-not-xml")
        self.assertNotEqual(rc, 0, "Malformed XML must fail")
        self.assertIn("Error", out)

    def test_missing_coverage_file_fails_closed(self):
        cmd = [
            sys.executable,
            self.enforce,
            "/definitely/not/a/real/covertura.xml",
            "--thresholds-json",
            "/dev/null",
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        self.assertNotEqual(proc.returncode, 0, "Missing coverage file must fail")
        self.assertIn("Error", proc.stdout)

    def test_invalid_threshold_argument_fails_closed(self):
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.99">
  <packages>
    <package name="contracts/common/src" line-rate="0.99"/>
  </packages>
</coverage>"""
        rc, out, err = self._run(xml_text, ["--threshold", "150"])
        self.assertNotEqual(rc, 0, "Out-of-range --threshold must fail")
        self.assertIn("Error", out)

    def test_concurrent_runs_are_deterministic(self):
        """Concurrent invocations on the same report must agree."""
        xml_text = """<?xml version="1.0"?>
<coverage line-rate="0.99">
  <packages>
    <package name="contracts/common/src" line-rate="0.99"/>
  </packages>
</coverage>"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".xml", delete=False) as f:
            f.write(xml_text)
            f.flush()
            xml_path = f.name

        cmd = [sys.executable, self.enforce, xml_path, "--thresholds-json", "/dev/null"]
        procs = [
            subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            for _ in range(4)
        ]
        results = [p.communicate() + (p.returncode,) for p in procs]
        os.unlink(xml_path)

        exit_codes = {r[2] for r in results}
        self.assertEqual(exit_codes, {0}, f"Concurrent runs disagreed: {results!r}")


if __name__ == "__main__":
    unittest.main()
