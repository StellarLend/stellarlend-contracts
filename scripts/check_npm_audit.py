#!/usr/bin/env python3
import json
import re
import sys
from pathlib import Path

GHSA_RE = re.compile(r"GHSA-[a-z0-9-]+", re.IGNORECASE)
BLOCKING_SEVERITIES = {"high", "critical"}


def accepted_advisories(config_path):
    accepted = set()
    for line in config_path.read_text(encoding="utf-8").splitlines():
        if not line.lstrip().startswith("#"):
            continue
        accepted.update(match.upper() for match in GHSA_RE.findall(line))
    return accepted


def reported_advisories(report_path):
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if "error" in report:
        raise ValueError(report["error"])
    reported = {}
    blocking = False
    for package, details in report.get("vulnerabilities", {}).items():
        if details.get("severity") not in BLOCKING_SEVERITIES:
            continue
        blocking = True
        for via in details.get("via", []):
            if not isinstance(via, dict):
                continue
            matches = {match.upper() for match in GHSA_RE.findall(via.get("url", ""))}
            if not matches:
                matches = {f"UNLISTED:{package}"}
            for advisory in matches:
                title = via.get("title", "unknown advisory")
                reported.setdefault(advisory, set()).add(f"{package}: {title}")
    if blocking and not reported:
        raise ValueError("npm audit reported high/critical vulnerabilities without advisory ids")
    return reported


def main(argv):
    if len(argv) != 3:
        print("usage: check_npm_audit.py AUDIT_TOML NPM_AUDIT_JSON", file=sys.stderr)
        return 2

    try:
        accepted = accepted_advisories(Path(argv[1]))
        reported = reported_advisories(Path(argv[2]))
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"npm audit check failed: {exc}", file=sys.stderr)
        return 1

    unexpected = sorted(set(reported) - accepted)
    if unexpected:
        print("Unaccepted high/critical npm advisories:", file=sys.stderr)
        for advisory in unexpected:
            print(f"  {advisory}", file=sys.stderr)
            for detail in sorted(reported[advisory]):
                print(f"    {detail}", file=sys.stderr)
        print(f"Add each advisory with justification to {argv[1]}.", file=sys.stderr)
        return 1

    print(f"npm audit: {len(reported)} high/critical advisories, all accepted in {argv[1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
