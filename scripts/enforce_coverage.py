#!/usr/bin/env python3
"""Enforce minimum coverage thresholds from cobertura.xml.

Reads per-crate thresholds from ``contracts/coverage_thresholds.json`` (with a
per-crate thresholds from ``scripts/coverage_thresholds.json`` (with a
``flat_threshold`` fallback for any crate not listed), iterates ``<package>``
elements in the Cobertura report, and fails with the offending crate name if
any crate is below its configured threshold.

Invariants enforced by this module:

* The configured threshold for a crate is deterministic and independent of
  the order in which packages appear in the report.
* Thresholds and line rates must be finite numbers in [0, 1]; any other value
  is a hard error rather than a silent pass/fail.
* A missing or unparseable config file falls back to documented defaults
  instead of silently passing.
* A report with no `<package>` elements fails closed.
* Repeated crate names are deduplicated to the worst observed coverage so
  duplicate entries cannot hide a failing crate.
* The overall line rate is always enforced against the flat threshold, even
  when ``--overall-only`` is used.
"""

import argparse
import json
import math
import os
import sys
import xml.etree.ElementTree as ET


DEFAULT_FLAT_THRESHOLD = 95.0


class CoverageError(ValueError):
    """Raised when a coverage input is invalid or inconsistent."""


def _parse_ratio(value, label):
    """Parse a line-rate string into a float in [0, 1].

    Raises `CoverageError` for non-numeric, non-finite, or out-of-range
    values so that corrupt reports cannot silently pass.
    """
    try:
        ratio = float(value)
    except (TypeError, ValueError) as exc:
        raise CoverageError(f"Invalid {label} value {value!r}: {exc}") from exc
    if not math.isfinite(ratio):
        raise CoverageError(f"Invalid {label} value {value!r}: not finite")
    if not 0.0 <= ratio <= 1.0:
        raise CoverageError(
            f"Invalid {label} value {value!r}: must be within [0, 1]"
        )
    return ratio


def _parse_threshold(value, label):
    """Parse a threshold expressed as a percentage in [0, 100]."""
    try:
        threshold = float(value)
    except (TypeError, ValueError) as exc:
        raise CoverageError(f"Invalid {label} threshold {value!r}: {exc}") from ex
    if not math.isfinite(threshold):
        raise CoverageError(f"Invalid {label} threshold {value!r}: not finite")
    if not 0.0 <= threshold <= 100.0:
        raise CoverageError(
            f"Invalid {label} threshold {value!r}: must be within [0, 100]"
        )
    return threshold


def _resolve_coverage_path(requested):
    """Locate the cobertura.xml file, falling back to known tarpaulin output dirs."""
    candidates = [requested]
    base_name = os.path.basename(requested)
    candidates.append(os.path.join(os.path.dirname(os.path.dirname(requested)), base_name))
    candidates.append(os.path.join(os.getcwd(), "stellar-lend", base_name))
    for path in candidates:
        if path and os.path.isfile(path):
            return path
    return requested


def load_thresholds(config_path):
    """Load coverage thresholds from a JSON config file.

    Returns a dict containing 'flat_threshold' (float) and 'per_crate' (dict of
    crate-name → float). Returns defaults if the file does not exist.

    A missing file is not an error (callers rely on the documented defaults),
    but a malformed or out-of-range file is rejected with `CoverageError` so a
    corrupt config cannot silently weaken enforcement.
    """
    defaults = {"flat_threshold": DEFAULT_FLAT_THRESHOLD, "per_crate": {}}
    if not config_path or not os.path.isfile(config_path):
        return defaults

    try:
        with open(config_path) as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError) as exc:
        raise CoverageError(f"Failed to load threshold config {config_path!r}: {exc}") from ex

    if not isinstance(data, dict):
        raise CoverageError(
            f"Threshold config {config_path!r} must be a JSON object"
        )

    flat = _parse_threshold(
        data.get("flat_threshold", defaults["flat_threshold"]),
        "flat",
    )

    per_crate_raw = data.get("per_crate", defaults["per_crate"])
    if not isinstance(per_crate_raw, dict):
        raise CoverageError(
            f"Threshold config {config_path!r}: 'per_crate' must be an object"
        )

    per_crate = {}
    for crate, value in per_crate_raw.items():
        if not isinstance(crate, str) or not crate:
            raise CoverageError(
                f"Threshold config {config_path!r}: crate names must be non-empty strings"
            )
        per_crate[crate] = _parse_threshold(value, f"crate {crate!r}")

    return {"flat_threshold": flat, "per_crate": per_crate}


def get_threshold(crate_name, thresholds):
    """Return the threshold for *crate_name*, falling back to flat_threshold."""
    per_crate = thresholds.get("per_crate", {})
    if crate_name in per_crate:
        return per_crate[crate_name]
    return thresholds.get("flat_threshold", DEFAULT_FLAT_THRESHOLD)


def _collect_packages(root):
    """Return a mapping of crate name → worst observed coverage percentage.

    Duplicate ``<package>`` elements for the same crate are collapsed to the
    lowest coverage value so a duplicate high-coverage entry cannot mask a failing
    one. Packages without a line-rate are reported as `None` and skipped.
    """
    packages = {}
    for pkg in root.findall(".//package"):
        name = pkg.get("name") or "unknown"
        line_rate_str = pkg.get("line-rate")
        if line_rate_str is None:
            packages.setdefault(name, None)
            continue
        coverage_pct = _parse_ratio(line_rate_str, f"line-rate for {name!r}") * 100.0
        previous = packages.get(name)
        if previous is None and name in packages:
            packages[name] = coverage_pct
        elif previous is None:
            packages[name] = coverage_pct
        else:
            packages[name] = min(previous, coverage_pct)
    return packages


def _run_check(coverage_file, thresholds, flat_threshold, overall_only):
    """Evaluate the report and return a list of failure tuples.

    Raises `CoverageError` for malformed inputs. Returns `[]` when every
    enforced threshold is met.
    """
    try:
        tree = ET.parse(coverage_file)
        root = tree.getroot()
    except (ET.ParseError, OSError) as exc:
        raise CoverageError(f"Error parsing {coverage_file}: {exc}") from ex

    packages = _collect_packages(root)
    if not packages:
        raise CoverageError("Error: No <package> elements found in cobertura.xml")

    failures = []
    print(f"{'Crate':<45} {'Coverage'>:>10} {'Threshold':>10}  Status")
    print("-" * 80)

    if overall_only:
        print(f"  { '(packages skipped by --overall-only)':<45} {'N/A'>:>10} {'N/A':>10}  SKIP")
    else:
        for name in sorted(packages):
            coverage_pct = packages[name]
            if coverage_pct is None:
                print(f"  {name:<45} {'N/A':>10} {'N/A':>10}  SKIP (no line-rate)")
                continue
            crate_threshold = get_threshold(name, thresholds)
            ok = coverage_pct >= crate_threshold
            status = "OK" if ok else "FAIL"
            print(f"  {name:<45} {coverage_pct:>9.2f}% {crate_threshold:>9.2f}%  {status}")
            if not ok:
                failures.append((name, coverage_pct, crate_threshold))

    overall_line_rate = root.attrib.get("line-rate")
    if overall_line_rate is not None:
        overall_pct = _parse_ratio(overall_line_rate, "overall line-rate") * 100.0
        overall_ok = overall_pct >= flat_threshold
        overall_status = "OK" if overall_ok else "FAIL"
        print(f"  {'(overall)':<45} {overall_pct:>9.2f}% {flat_threshold:>9.2f}%  {overall_status}")
        if not overall_ok:
            failures.append(("(overall)", overall_pct, flat_threshold))

    return failures


def main(argv_list=None):
    parser = argparse.ArgumentParser(
        description="Enforce minimum coverage thresholds from cobertura.xml"
    )
    parser.add_argument("coverage_file", help="Path to cobertura.xml")
    parser.add_argument(
        "--threshold",
        type=float,
        default=None,
        help="Override flat threshold (overrides JSON config)",
    )
    parser.add_argument(
        "--thresholds-json",
        type=str,
        default=None,
        help="Path to coverage_thresholds.json (default: scripts/coverage_thresholds.json)",
    )
    parser.add_argument(
        "--overall-only",
        action="store_true",
        help="Only enforce the report-level overall line-rate.",
    )
    args = parser.parse_args(argv_list)

    coverage_file = _resolve_coverage_path(args.coverage_file)

    thresholds_json = args.thresholds_json
    if thresholds_json is None:
        script_dir = os.path.dirname(os.path.abspath(__file__))
        thresholds_json = os.path.join(script_dir, "coverage_thresholds.json")

    try:
        thresholds = load_thresholds(thresholds_json)
        flat_threshold = (
            _parse_threshold(args.threshold, "--threshold")
            if args.threshold is not None
            else thresholds["flat_threshold"]
        )
        failures = _run_check(coverage_file, thresholds, flat_threshold, args.overall_only)
    except CoverageError as exc:
        print(f"Error: {exc}")
        sys.exit(1)

    print()
    if failures:
        print("Coverage check FAILED:")
        for name, got, expected in failures:
            print(f"  {name}: {got:.2f}% < {expected:.2f}%")
        sys.exit(1)

    print("Coverage check passed!")


if __name__ == "__main__":
    main()
