from pathlib import Path

# Expose the shared canonical helper and the new evidence module.
p = Path("crates/prospect-evidence/src/lib.rs")
s = p.read_text()
anchor = "#![forbid(unsafe_code)]\n\npub mod constrained;\n"
replacement = "#![forbid(unsafe_code)]\n\nmod canonical;\npub mod constrained;\npub mod multi_trace;\n"
if s.count(anchor) != 1:
    raise SystemExit("prospect-evidence module anchor drift")
p.write_text(s.replace(anchor, replacement, 1))

# Make constrained evidence consume the shared recursive canonicalizer.
p = Path("crates/prospect-evidence/src/constrained.rs")
s = p.read_text()
import_anchor = "use super::{EvidenceError, EvidenceNature, EvidenceSource, RunId};\n"
if s.count(import_anchor) != 1:
    raise SystemExit("constrained import anchor drift")
s = s.replace(
    import_anchor,
    "use super::canonical::to_canonical_json;\nuse super::{EvidenceError, EvidenceNature, EvidenceSource, RunId};\n",
    1,
)
start = s.find("fn canonical_json_value(\n")
end_marker = "impl<R, T> ConstrainedDecisionEvidence<R, T>\n"
end = s.find(end_marker, start)
if start == -1 or end == -1:
    raise SystemExit("constrained canonical helper block drift")
s = s[:start] + s[end:]
p.write_text(s)

# Wire the exact multi-trace scenario module copied from the already-qualified #52 branch.
p = Path("crates/prospect-scenario/src/lib.rs")
s = p.read_text()
anchor = "pub mod controlled;\npub mod decision;\n"
if s.count(anchor) != 1:
    raise SystemExit("prospect-scenario module anchor drift")
p.write_text(s.replace(anchor, anchor + "pub mod multi_trace;\n", 1))
