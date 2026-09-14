"""Contract tests only; no model runs and no observed metrics."""

import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verify_r2_preflight_receipt import (
    IDENTITY, MAX_BYTES, ReceiptError, canonical_json, expected_campaigns, verify_receipt,
)


def fixture():
    return dict(IDENTITY, device_ordinal=0, campaigns=expected_campaigns())


def encode(value):
    return canonical_json(value).encode("utf-8")


class ReceiptTests(unittest.TestCase):
    def test_accepts_exact_input_only_receipt_and_hashes_actual_bytes(self):
        for ending in (b"", b"\n"):
            payload = encode(fixture()) + ending
            result = verify_receipt(payload)
            self.assertEqual(result["campaign_count"], 3)
            self.assertEqual(result["evidence_kind"], "preflight_only")
            self.assertEqual(result["receipt_sha256"], hashlib.sha256(payload).hexdigest())

    def test_rejects_every_changed_identity_field(self):
        for field in IDENTITY:
            with self.subTest(field=field):
                value = fixture()
                value[field] = "altered"
                with self.assertRaises(ReceiptError):
                    verify_receipt(encode(value))

    def test_rejects_noncanonical_unknown_and_observed_fields(self):
        for payload in (json.dumps(fixture(), indent=2).encode(), encode(fixture()) + b"\n\n",
                        b"[]", b"garbage", b"\xff", b" " * (MAX_BYTES + 1),
                        encode(dict(fixture(), observations=[])),
                        encode(dict(fixture(), schema="kvlab.smollm2-r2-position-suite-result/v1"))):
            with self.subTest(payload=payload[:50]), self.assertRaises(ReceiptError):
                verify_receipt(payload)

    def test_rejects_bad_device_types_and_ranges(self):
        for device in (True, -1, 0.0, "0", 2**31, None):
            with self.subTest(device=device), self.assertRaises(ReceiptError):
                verify_receipt(encode(dict(fixture(), device_ordinal=device)))

    def test_rejects_campaign_reordering_missing_entries_and_extra_fields(self):
        campaigns = expected_campaigns()
        alternatives = [list(reversed(campaigns)), campaigns[:2], campaigns + [campaigns[0]],
                        [dict(campaigns[0], metrics=[]), *campaigns[1:]]]
        for altered in alternatives:
            with self.subTest(altered=altered), self.assertRaises(ReceiptError):
                verify_receipt(encode(dict(fixture(), campaigns=altered)))

    def test_rejects_digest_policy_path_and_numeric_type_drift(self):
        for field, value in (("campaign_spec_sha256", "0" * 64), ("trace_sha256", "0" * 64),
                             ("policies", ["random_seeded", "lru"]), ("campaign_path", "../other"),
                             ("retained_count", 7.0)):
            altered = copy.deepcopy(fixture())
            altered["campaigns"][0][field] = value
            with self.subTest(field=field), self.assertRaises(ReceiptError):
                verify_receipt(encode(altered))


if __name__ == "__main__":
    unittest.main()
