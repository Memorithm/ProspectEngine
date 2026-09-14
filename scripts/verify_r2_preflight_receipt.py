"""Check a pinned KVLab R2 preflight receipt, never an observed-model result."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys
from typing import Any

SCHEMA = "kvlab.smollm2-r2-position-suite-preflight/v1"
IDENTITY = {
    "schema": SCHEMA,
    "kvlab_preregistration_revision": "216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5",
    "kvlab_execution_revision": "404577ce939093767dc75d2d67de2fe3c16fa4dc",
    "nnis_runtime_revision": "091aabbb3e132627cf64716720aae530442d2a32",
    "prospect_verifier_revision": "298acdc91682ef1d09914b6f964e8934828825c0",
    "model_id": "HuggingFaceTB/SmolLM2-135M",
    "model_revision": "93efa2f097d58c2a74874c7e644dbc9b0cee75a2",
    "source_model_sha256": "80521b40281d6ce74e35c9282c22539e75aa0ac8578892b2a59955ef78d55da1",
    "runtime_backend": "nnis-kvlab-v4",
    "bytes_per_token": 46080,
}
DIGESTS = {
    7: "d826e0ca1869b6f3134e8b34bb65db14aa034d2518bf9559810ae80f19012346",
    14: "01eeec54e02bf3f56bbd2e75175e2f04fc4593701875a21b90981b40cfa4eff5",
    20: "e09b14c8479bac98b93625f3d98f667943d8318ff769dbbbf3db989959dcba07",
}
TRACE = "3411f378fb3c7010eb94361128c019206fb47bda4271f721498d517eb07ca65f"
MAX_BYTES = 65536


class ReceiptError(ValueError):
    """A readiness receipt does not satisfy the frozen input-only contract."""


def canonical_json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def expected_campaigns() -> list[dict[str, Any]]:
    return [dict(
        retained_count=count,
        campaign_path=f"experiments/prospect/smollm2-r2/retain-{count:02d}-of-27.json",
        campaign_spec_sha256=digest, trace_sha256=TRACE,
        policies=["lru", "random_seeded"],
    ) for count, digest in DIGESTS.items()]


def verify_receipt(payload: bytes) -> dict[str, Any]:
    if len(payload) > MAX_BYTES:
        raise ReceiptError("readiness receipt exceeds size limit")
    try:
        text = payload.decode("utf-8")
        def reject_constant(value):
            raise ReceiptError(f"non-finite JSON value: {value}")
        value = json.loads(text, parse_constant=reject_constant)
        canonical = canonical_json(value)
    except (UnicodeDecodeError, ValueError, RecursionError) as error:
        raise ReceiptError("invalid UTF-8 readiness JSON") from error
    if not isinstance(value, dict):
        raise ReceiptError("readiness receipt must be an object")
    # The launcher CLI adds exactly one optional final LF; no other whitespace
    # or duplicate key is accepted as canonical content.
    if text not in (canonical, canonical + "\n"):
        raise ReceiptError("readiness receipt is not canonical")
    if set(value) != set(IDENTITY) | {"device_ordinal", "campaigns"}:
        raise ReceiptError("unexpected readiness fields")
    for key, expected in IDENTITY.items():
        if type(value[key]) is not type(expected) or value[key] != expected:
            raise ReceiptError(f"readiness identity mismatch: {key}")
    device = value["device_ordinal"]
    if type(device) is not int or not 0 <= device <= 2**31 - 1:
        raise ReceiptError("device ordinal must be a non-negative i32")
    # Compare canonical encodings, not Python equality (7.0 == 7 is true).
    if canonical_json(value["campaigns"]) != canonical_json(expected_campaigns()):
        raise ReceiptError("readiness campaign identities/order differ from preregistration")
    return {
        "schema": "prospect.r2-readiness-receipt-check/v1",
        "receipt_sha256": hashlib.sha256(payload).hexdigest(),
        "campaign_count": len(DIGESTS),
        "evidence_kind": "preflight_only",
        "scope": "declared_input_and_tool_identities_only",
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("receipt", type=Path)
    args = parser.parse_args(argv)
    try:
        with args.receipt.open("rb") as stream:
            result = verify_receipt(stream.read(MAX_BYTES + 1))
    except (OSError, ReceiptError) as error:
        print(str(error), file=sys.stderr)
        return 1
    print(canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
