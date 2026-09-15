from pathlib import Path

script = Path('.ci/tmp_prepare_continuation.py')
source = script.read_text()
old = '''source = recovery.read_text()
recovery.write_text(source.replace(
    "//! Read-only restart admission against caller-trusted external expectations.\\n",
    "//! Read-only restart admission against caller-trusted external expectations.\\n\\npub mod continuation;\\n",
    1,
))'''
new = '''source = recovery.read_text()
anchor = "use serde::{Deserialize, Serialize};"
if source.count(anchor) != 1:
    raise SystemExit("recovery.rs module insertion anchor drift")
recovery.write_text(source.replace(anchor, "pub mod continuation;\\n\\n" + anchor, 1))'''
if source.count(old) != 1:
    raise SystemExit('preparation script patch anchor drift')
script.write_text(source.replace(old, new, 1))
exec(compile(script.read_text(), str(script), 'exec'))
