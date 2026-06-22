#!/usr/bin/env python3
"""Enforce minimum coverage thresholds from cobertura.xml.

Reads per-crate thresholds from ``scripts/coverage_thresholds.json`` (with a
``flat_threshold`` fallback for any crate not listed), iterates ``<package>``
elements in the Cobertura report, and fails with the offending crate name if
any crate is below its configured threshold.
"""

import argparse
import json
import os
import sys
import xml.etree.ElementTree as ET


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
    """
    defaults = {"flat_threshold": 95.0, "per_crate": {}}
    if config_path and os.path.isfile(config_path):
        with open(config_path) as f:
            data = json.load(f)
        return {
            "flat_threshold": data.get("flat_threshold", defaults["flat_threshold"]),
            "per_crate": data.get("per_crate", defaults["per_crate"]),
        }
    return defaults


def get_threshold(crate_name, thresholds):
    """Return the threshold for *crate_name*, falling back to flat_threshold."""
    per_crate = thresholds.get("per_crate", {})
    if crate_name in per_crate:
        return per_crate[crate_name]
    return thresholds.get("flat_threshold", 95.0)


def main():
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
    args = parser.parse_args()

    coverage_file = _resolve_coverage_path(args.coverage_file)

    try:
        tree = ET.parse(coverage_file)
        root = tree.getroot()
    except Exception as e:
        print(f"Error parsing {coverage_file}: {e}")
        sys.exit(1)

    thresholds_json = args.thresholds_json
    if thresholds_json is None:
        script_dir = os.path.dirname(os.path.abspath(__file__))
        thresholds_json = os.path.join(script_dir, "coverage_thresholds.json")
    thresholds = load_thresholds(thresholds_json)

    flat_threshold = args.threshold if args.threshold is not None else thresholds["flat_threshold"]

    packages = root.findall(".//package")
    if not packages:
        print("Error: No <package> elements found in cobertura.xml")
        sys.exit(1)

    failures = []
    print(f"{'Crate':<45} {'Coverage':>10} {'Threshold':>10}  Status")
    print("-" * 80)

    if args.overall_only:
        print(f"  {'(packages skipped by --overall-only)':<45} {'N/A':>10} {'N/A':>10}  SKIP")
    else:
        for pkg in packages:
            name = pkg.get("name", "unknown")
            line_rate_str = pkg.get("line-rate")
            if line_rate_str is None:
                print(f"  {name:<45} {'N/A':>10} {'N/A':>10}  SKIP (no line-rate)")
                continue
            coverage_pct = float(line_rate_str) * 100
            crate_threshold = get_threshold(name, thresholds)
            ok = coverage_pct >= crate_threshold
            status = "OK" if ok else "FAIL"
            print(f"  {name:<45} {coverage_pct:>9.2f}% {crate_threshold:>9.2f}%  {status}")
            if not ok:
                failures.append((name, coverage_pct, crate_threshold))

    overall_line_rate = root.attrib.get("line-rate")
    if overall_line_rate is not None:
        overall_pct = float(overall_line_rate) * 100
        overall_ok = overall_pct >= flat_threshold
        overall_status = "OK" if overall_ok else "FAIL"
        print(f"  {'(overall)':<45} {overall_pct:>9.2f}% {flat_threshold:>9.2f}%  {overall_status}")
        if not overall_ok:
            failures.append(("(overall)", overall_pct, flat_threshold))

    print()
    if failures:
        print("Coverage check FAILED:")
        for name, got, expected in failures:
            print(f"  {name}: {got:.2f}% < {expected:.2f}%")
        sys.exit(1)

    print("Coverage check passed!")


if __name__ == "__main__":
    main()
