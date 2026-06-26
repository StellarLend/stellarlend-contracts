#!/usr/bin/env python3
"""Unit tests for enforce_coverage.py."""

import json
import os
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from enforce_coverage import load_thresholds, get_threshold, _resolve_coverage_path


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
            f.write(b"<xml/>")
            f.flush()
            result = _resolve_coverage_path(f.name)
            self.assertEqual(result, f.name)
        os.unlink(f.name)

    def test_nonexistent_file_returns_input(self):
        result = _resolve_coverage_path("/definitely/does/not/exist.xml")
        self.assertEqual(result, "/definitely/does/not/exist.xml")


class TestMainIntegration(unittest.TestCase):
    def setUp(self):
        self.script_dir = os.path.join(os.path.dirname(__file__), "..")
        self.enforce = os.path.join(self.script_dir, "enforce_coverage.py")

    def _run(self, coverage_xml, extra_args=None):
        """Run enforce_coverage.py with a temp cobertura.xml, return (exit_code, output)."""
        import subprocess

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


if __name__ == "__main__":
    unittest.main()
