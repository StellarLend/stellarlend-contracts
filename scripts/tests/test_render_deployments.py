#!/usr/bin/env python3
"""Unit tests for scripts/render_deployments.py"""

import json
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

# Make the scripts directory importable.
sys.path.insert(0, str(Path(__file__).parent.parent))
from render_deployments import load_and_validate, render, CONTRACT_ID_RE


VALID_ID   = "C" + "A" * 55
VALID_DATA = {
    "network": "testnet",
    "contracts": {
        "stellarlend-lending": {
            "id": VALID_ID,
            "explorer": f"https://stellar.expert/explorer/testnet/contract/{VALID_ID}",
            "status": "current",
        }
    },
    "admin": "GADMIN",
    "demo_user": "GDEMO",
}


class TestContractIdRegex(unittest.TestCase):
    def test_valid_id_matches(self):
        self.assertRegex(VALID_ID, CONTRACT_ID_RE)

    def test_too_short_rejected(self):
        self.assertNotRegex("C" + "A" * 54, CONTRACT_ID_RE)

    def test_too_long_rejected(self):
        self.assertNotRegex("C" + "A" * 56, CONTRACT_ID_RE)

    def test_wrong_first_char_rejected(self):
        self.assertNotRegex("G" + "A" * 55, CONTRACT_ID_RE)

    def test_lowercase_rejected(self):
        self.assertNotRegex("C" + "a" * 55, CONTRACT_ID_RE)


class TestRender(unittest.TestCase):
    def test_renders_contract_id_in_table(self):
        md = render(VALID_DATA)
        self.assertIn(VALID_ID, md)

    def test_renders_status_in_label(self):
        md = render(VALID_DATA)
        self.assertIn("current", md)

    def test_renders_network(self):
        md = render(VALID_DATA)
        self.assertIn("testnet", md)

    def test_renders_wasm_hash_when_present(self):
        data = json.loads(json.dumps(VALID_DATA))
        data["contracts"]["stellarlend-lending"]["wasm_hash"] = "abc123"
        md = render(data)
        self.assertIn("abc123", md)

    def test_no_wasm_column_empty_when_absent(self):
        md = render(VALID_DATA)
        # wasm_hash not set so the column cell should be empty
        lines = [l for l in md.splitlines() if VALID_ID in l]
        self.assertTrue(any(l.strip().endswith("|  |") or l.strip().endswith("| |") for l in lines))


class TestLoadAndValidate(unittest.TestCase):
    def _write_and_load(self, data, tmp_path):
        p = tmp_path / "DEPLOYMENTS.json"
        p.write_text(json.dumps(data))
        with patch("render_deployments.JSON_PATH", p):
            return load_and_validate(p)

    def test_valid_data_passes(self):
        import tempfile, os
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "DEPLOYMENTS.json"
            p.write_text(json.dumps(VALID_DATA))
            result = load_and_validate(p)
            self.assertEqual(result["network"], "testnet")

    def test_invalid_id_exits(self):
        import tempfile
        bad = json.loads(json.dumps(VALID_DATA))
        bad["contracts"]["stellarlend-lending"]["id"] = "NOTVALID"
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "DEPLOYMENTS.json"
            p.write_text(json.dumps(bad))
            with self.assertRaises(SystemExit) as cm:
                load_and_validate(p)
            self.assertEqual(cm.exception.code, 1)

    def test_missing_status_exits(self):
        import tempfile
        bad = json.loads(json.dumps(VALID_DATA))
        del bad["contracts"]["stellarlend-lending"]["status"]
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "DEPLOYMENTS.json"
            p.write_text(json.dumps(bad))
            with self.assertRaises(SystemExit) as cm:
                load_and_validate(p)
            self.assertEqual(cm.exception.code, 1)


if __name__ == "__main__":
    unittest.main()
