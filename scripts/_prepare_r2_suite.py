"""One-use source preparation, removed before PR validation and merge."""
from pathlib import Path
import hashlib


def checked_read(path, expected):
    data = Path(path).read_bytes()
    actual = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
    if actual != expected:
        raise SystemExit(f"base source changed: {path}")
    return data.decode()


def replace(source, old, new, count=1):
    if source.count(old) != count:
        raise SystemExit(f"expected {count} occurrences of {old!r}, got {source.count(old)}")
    return source.replace(old, new)


path = 'crates/prospect-cli/src/kv_campaign_suite.rs'
source = checked_read(path, '50047fed0f1c2cfa1028d4f6bf01fc9501d2067b')
production, tests = source.split('#[cfg(test)]\nmod tests {', 1)
profiles = '''// Profiles are fixed software contracts, never supplied by an untrusted manifest.
// R1 keeps its original pins; R2 cannot silently fall back to the R1 contract.
struct SuiteContract {
    input_schema: &'static str,
    verification_schema: &'static str,
    provider_revision: &'static str,
    preregistration_revision: &'static str,
    runtime_revision: &'static str,
    launch_verifier_revision: &'static str,
    campaign_root: &'static str,
    campaign_sha256: [&'static str; 3],
    generation: &'static str,
}

const R1_CONTRACT: SuiteContract = SuiteContract {
    input_schema: SUITE_SCHEMA_V1,
    verification_schema: VERIFICATION_SCHEMA_V1,
    provider_revision: KVLAB_SUITE_PROVIDER_REVISION,
    preregistration_revision: KVLAB_PREREGISTRATION_REVISION,
    runtime_revision: NNIS_RUNTIME_REVISION,
    launch_verifier_revision: PROSPECT_LAUNCH_VERIFIER_REVISION,
    campaign_root: "experiments/prospect/smollm2-r1",
    campaign_sha256: PREREGISTERED_CAMPAIGN_SHA256,
    generation: "r1",
};

const R2_CONTRACT: SuiteContract = SuiteContract {
    input_schema: "kvlab.smollm2-r2-position-suite-result/v1",
    verification_schema: "prospect.kv-campaign-suite-r2-verification/v1",
    provider_revision: "cefe129f126c865819545ea94b3d3510400d8964",
    preregistration_revision: "216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5",
    runtime_revision: "091aabbb3e132627cf64716720aae530442d2a32",
    launch_verifier_revision: "298acdc91682ef1d09914b6f964e8934828825c0",
    campaign_root: "experiments/prospect/smollm2-r2",
    campaign_sha256: [
        "d826e0ca1869b6f3134e8b34bb65db14aa034d2518bf9559810ae80f19012346",
        "01eeec54e02bf3f56bbd2e75175e2f04fc4593701875a21b90981b40cfa4eff5",
        "e09b14c8479bac98b93625f3d98f667943d8318ff769dbbbf3db989959dcba07",
    ],
    generation: "r2",
};

'''
# generation is also used in production to bind each exact experiment name.
production = replace(production, '#[derive(Clone, Debug, PartialEq, Serialize)]\npub struct BaselineMetricSummary', profiles + '#[derive(Clone, Debug, PartialEq, Serialize)]\npub struct BaselineMetricSummary')
production = replace(production, '''    let directory = directory.as_ref();
    let manifest_path = directory.join("suite-manifest.json");''', '''    verify_suite_with_contract(directory.as_ref(), &R1_CONTRACT)
}

/// Independently verify the frozen R2 suite. Does not execute a model.
pub fn verify_kv_campaign_suite_r2_directory(
    directory: impl AsRef<Path>,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {
    verify_suite_with_contract(directory.as_ref(), &R2_CONTRACT)
}

fn verify_suite_with_contract(
    directory: &Path,
    contract: &SuiteContract,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {
    let manifest_path = directory.join("suite-manifest.json");''')
production = replace(production, 'validate_manifest(&manifest)?;', 'validate_manifest(&manifest, contract)?;')
production = replace(production, 'verify_campaign_context(&campaign_directory, &manifest, entry)?;', 'verify_campaign_context(&campaign_directory, &manifest, entry, contract)?;')
production = replace(production, '        let summary = verify_kv_campaign_directory(&campaign_directory)', '        validate_campaign_entry_types(&campaign_directory)?;\n        let summary = verify_kv_campaign_directory(&campaign_directory)')
production = replace(production, 'schema: VERIFICATION_SCHEMA_V1.to_owned(),', 'schema: contract.verification_schema.to_owned(),')
production = replace(production, 'kvlab_suite_provider_revision: KVLAB_SUITE_PROVIDER_REVISION.to_owned(),', 'kvlab_suite_provider_revision: contract.provider_revision.to_owned(),')
production = replace(production, 'fn validate_manifest(manifest: &SuiteManifestWire) -> Result<(), KvCampaignSuiteError> {', 'fn validate_manifest(manifest: &SuiteManifestWire, contract: &SuiteContract) -> Result<(), KvCampaignSuiteError> {')
production = replace(production, 'manifest.schema != SUITE_SCHEMA_V1', 'manifest.schema != contract.input_schema')
for old, new in [('            KVLAB_PREREGISTRATION_REVISION,', '            contract.preregistration_revision,'), ('            NNIS_RUNTIME_REVISION,', '            contract.runtime_revision,'), ('            PROSPECT_LAUNCH_VERIFIER_REVISION,', '            contract.launch_verifier_revision,')]:
    production = replace(production, old, new)
production = replace(production, 'let _device_ordinal = manifest.device_ordinal;', '''if manifest.device_ordinal > i32::MAX as usize {
        return Err(KvCampaignSuiteError::InvalidManifest("device_ordinal"));
    }''')
production = replace(production, 'PREREGISTERED_CAMPAIGN_SHA256[index]', 'contract.campaign_sha256[index]')
production = replace(production, 'format!("experiments/prospect/smollm2-r1/{stem}.json")', 'format!("{}/{stem}.json", contract.campaign_root)')
production = replace(production, '''    entry: &SuiteCampaignWire,
) -> Result<(), KvCampaignSuiteError> {
    let payload = read_text(&campaign_directory.join("campaign.json"))?;''', '''    entry: &SuiteCampaignWire,
    contract: &SuiteContract,
) -> Result<(), KvCampaignSuiteError> {
    let payload = read_text(&campaign_directory.join("campaign.json"))?;''')
production = replace(production, '''    if campaign.experiment_id.trim().is_empty() || campaign.evaluation_id.trim().is_empty() {''', '''    let expected_experiment = format!(
        "smollm2-{}-position-retain-{:02}-of-27", contract.generation, entry.retained_count
    );
    if campaign.experiment_id != expected_experiment || campaign.evaluation_id.trim().is_empty() {''')
production = replace(production, '''fn read_text(path: &Path) -> Result<String, KvCampaignSuiteError> {
    fs::read_to_string(path)''', '''// The input directory must remain trusted and unmodified during verification.
// These checks reject static symlinks/special files; they are not an OS sandbox.
fn require_regular_file(path: &Path) -> Result<(), KvCampaignSuiteError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvCampaignSuiteError::Io {
        path: path.to_path_buf(), source,
    })?;
    if !metadata.is_file() {
        return Err(KvCampaignSuiteError::EntryTypeMismatch(path.display().to_string()));
    }
    Ok(())
}

fn validate_campaign_entry_types(directory: &Path) -> Result<(), KvCampaignSuiteError> {
    let entries = fs::read_dir(directory).map_err(|source| KvCampaignSuiteError::Io {
        path: directory.to_path_buf(), source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| KvCampaignSuiteError::Io {
            path: directory.to_path_buf(), source,
        })?;
        require_regular_file(&entry.path())?;
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, KvCampaignSuiteError> {
    require_regular_file(path)?;
    fs::read_to_string(path)''')
# Reuse the existing fixture generator for both profiles; retain all R1 tests.
tests = replace(tests, '''    fn write_suite_variant(directory: &Path, baseline_drift: bool, drift: Option<&str>) {
        let mut suite_entries = Vec::new();''', '''    fn write_suite_variant(directory: &Path, baseline_drift: bool, drift: Option<&str>) {
        write_suite_for_contract(directory, baseline_drift, drift, &R1_CONTRACT);
    }

    fn write_suite_for_contract(directory: &Path, baseline_drift: bool, drift: Option<&str>, contract: &SuiteContract) {
        let mut suite_entries = Vec::new();''')
tests = replace(tests, 'write_campaign(&campaign_dir, retained_count, baseline_char, drift);', 'write_campaign(&campaign_dir, retained_count, baseline_char, drift, contract);')
tests = replace(tests, 'format!("experiments/prospect/smollm2-r1/{stem}.json")', 'format!("{}/{stem}.json", contract.campaign_root)')
for old, new in [('"schema":SUITE_SCHEMA_V1', '"schema":contract.input_schema'), ('"kvlab_preregistration_revision":KVLAB_PREREGISTRATION_REVISION', '"kvlab_preregistration_revision":contract.preregistration_revision'), ('"nnis_runtime_revision":NNIS_RUNTIME_REVISION', '"nnis_runtime_revision":contract.runtime_revision'), ('"prospect_verifier_revision":PROSPECT_LAUNCH_VERIFIER_REVISION', '"prospect_verifier_revision":contract.launch_verifier_revision')]:
    tests = replace(tests, old, new)
tests = replace(tests, '        drift: Option<&str>,\n    ) -> (String, String, CampaignVerificationSummary)', '        drift: Option<&str>,\n        contract: &SuiteContract,\n    ) -> (String, String, CampaignVerificationSummary)')
tests = replace(tests, 'format!("smollm2-r1-position-retain-{retained_count:02}-of-27")', 'format!("smollm2-{}-position-retain-{retained_count:02}-of-27", contract.generation)')
tests = replace(tests, '"runtime_revision":NNIS_RUNTIME_REVISION', '"runtime_revision":contract.runtime_revision', count=2)
new_tests = Path('scripts/_r2_suite_tests.rs').read_text()
tests = replace(tests, '    fn write_suite(directory: &Path, baseline_drift: bool) {', new_tests + '\n    fn write_suite(directory: &Path, baseline_drift: bool) {')
Path(path).write_text(production + '#[cfg(test)]\nmod tests {' + tests)

path = 'crates/prospect-cli/src/main.rs'
main = checked_read(path, '8bc18884357ec7907fcbee5a259d6f83e93cd1d4')
main = replace(main, 'use kv_campaign_suite::verify_kv_campaign_suite_directory;', 'use kv_campaign_suite::{verify_kv_campaign_suite_directory, verify_kv_campaign_suite_r2_directory};')
main = replace(main, r'  prospect verify-kv-campaign-suite <suite-directory>\n', r'  prospect verify-kv-campaign-suite <suite-directory>\n  prospect verify-kv-campaign-suite-r2 <suite-directory>\n')
main = replace(main, '        Some("verify-kv-campaign-suite") => {', '''        Some("verify-kv-campaign-suite-r2") => {
            let directory = exactly_one_argument(&mut arguments, "verify-kv-campaign-suite-r2", "suite directory")?;
            let summary = verify_kv_campaign_suite_r2_directory(directory)
                .map_err(|error| CliError::Verification(error.to_string()))?;
            serde_json::to_string(&summary).map_err(|error| {
                CliError::Verification(format!("failed to encode R2 suite summary: {error}"))
            })
        }
        Some("verify-kv-campaign-suite") => {''')
main = replace(main, '    #[test]\n    fn rejects_unknown_command() {', '''    #[test]
    fn r2_suite_requires_exactly_one_directory() {
        for args in [vec!["prospect", "verify-kv-campaign-suite-r2"],
                     vec!["prospect", "verify-kv-campaign-suite-r2", "a", "b"]] {
            assert!(matches!(run(args.into_iter().map(OsString::from)), Err(CliError::Usage(_))));
        }
    }

    #[test]
    fn r2_suite_missing_directory_is_a_verification_error() {
        assert!(matches!(run(["prospect", "verify-kv-campaign-suite-r2", ""].map(OsString::from)), Err(CliError::Verification(_))));
    }

    #[test]
    fn rejects_unknown_command() {''')
Path(path).write_text(main)
print('R2 verifier, shared checks and regression tests prepared; compilation still required')
