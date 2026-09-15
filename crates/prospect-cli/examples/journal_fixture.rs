//! Software fixture for live file persistence, including a process exit mid-call.
//! No model, GPU, scientific metric or physical intervention is executed.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use prospect_adapter::{AdapterCapability, AdapterMetadata, AdapterMetadataError, ContractVersion, VersionedAdapter};
use prospect_bundle::{AdapterBinding, BundleScenario, ScenarioBundle};
use prospect_cli::execution_journal::{FileJournal, inspect_execution_journal_files};
use prospect_core::{ProspectiveEngine, ScenarioId};
use prospect_dispatch::execution::ExecutableAdapterRegistry;
use prospect_dispatch::execution::record::{PayloadCodecs};
use prospect_dispatch::execution::record::journal::{EngineIdentity, JournalCapture, evaluate_registered_bundle_journaled};
use prospect_evidence::RunId;
use prospect_registry::{MetricRegistry, DecisionPolicyRegistry};
use prospect_scenario::controlled::EvaluationControl;
use sha2::{Digest, Sha256};

struct Fixture { mode: String }
impl ProspectiveEngine<i32, i32> for Fixture {
    type Signature = i32;
    type Error = &'static str;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> { Ok(*state) }
    fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
        if *intervention == 2 {
            if self.mode == "--exit-during-second" {
                // Deliberately bypass Rust destruction after the durable intent.
                std::process::exit(23);
            }
            if self.mode == "--fail-second" { return Err("fixture failure, not a model failure"); }
        }
        Ok(state + intervention)
    }
}
impl VersionedAdapter for Fixture {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        AdapterMetadata::new("example.journal", version(), None,
            vec![AdapterCapability::new("example.evaluate", version())?])
    }
}
fn version() -> ContractVersion { ContractVersion::new(1, 0).unwrap() }
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() < 3 || args.len() > 4 {
        return Err("usage: journal_fixture <new-journal> <new-bundle> <declared-git-sha> [--exit-during-second|--fail-second]".into());
    }
    let journal_path = PathBuf::from(&args[0]);
    let bundle_path = PathBuf::from(&args[1]);
    let revision = args[2].to_str().ok_or("revision is not UTF-8")?;
    let mode = args.get(3).map(|v| v.to_str().ok_or("mode is not UTF-8")).transpose()?.unwrap_or("");
    if !["", "--exit-during-second", "--fail-second"].contains(&mode) { return Err("unknown fixture mode".into()); }
    // Hash the actual executable. Git revision is supplied explicitly, not inferred.
    let mut file = std::fs::File::open(std::env::current_exe()?)?;
    let mut hash = Sha256::new(); let mut bytes = [0_u8; 8192];
    loop { let n = file.read(&mut bytes)?; if n == 0 { break; } hash.update(&bytes[..n]); }
    let identity = EngineIdentity::new("example.journal_fixture", revision, &format!("{:x}", hash.finalize()))?;
    let fixture = Fixture { mode: mode.into() };
    let bundle = ScenarioBundle::new("example.journal_run", AdapterBinding::from_metadata(&fixture.adapter_metadata()?),
        Some(7), 10, (1..=3).map(|i| BundleScenario::new(ScenarioId::new(format!("s{i}")).unwrap(), i)).collect(), None, None)?;
    let mut adapters = ExecutableAdapterRegistry::new(); adapters.register(fixture)?;
    let mut options = OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut input_file = options.open(&bundle_path)?;
    input_file.write_all(bundle.canonical_json()?.as_bytes())?; input_file.sync_all()?; drop(input_file);
    let mut journal = FileJournal::create(&journal_path)?;
    let run = evaluate_registered_bundle_journaled(&bundle, &adapters, &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(), &EvaluationControl::new(3),
        JournalCapture::new(&mut journal, RunId::new("fixture-run")?, identity,
            PayloadCodecs::new("example.i32.v1", "example.error.v1")?,
            |s: &i32| Ok(s.to_string()), |e: &&str| Ok((*e).into())))
        .map_err(|e| io::Error::other(e.to_string()))?;
    if let Some(error) = run.journal_error() { return Err(io::Error::other(error.to_string()).into()); }
    drop(journal);
    println!("{}", serde_json::to_string(&inspect_execution_journal_files(journal_path, bundle_path)?)?);
    Ok(())
}
