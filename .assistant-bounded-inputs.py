"""One-use, blob-checked integration; removed before the final PR."""
from pathlib import Path
import hashlib
import sys

ROOT = Path('crates/prospect-cli/src')
ORIGINALS = {
    'lib.rs': '56db4d4398168a67e4de90ca352b0d69f983ba03',
    'kv_campaign_suite.rs': 'f6f2ba3b9dbb948b1cac272ae45e74abee79f11e',
    'kv_campaign_spec.rs': '69dfb486e2e3cf6fc04f3f84715448f52832f80d',
    'dispatch_preflight.rs': '47e679a05dc923be4c16073d7b6a485833673c39',
    'scenario_bundle.rs': '479646753e349045977c2c58c54edc3b78f23005',
}

def replace(source, old, new, count=1):
    if source.count(old) != count:
        raise RuntimeError(f'expected {count} occurrences: {old!r}, got {source.count(old)}')
    return source.replace(old, new)

def read(name):
    return (ROOT / name).read_text()

def write(name, source):
    (ROOT / name).write_text(source)

REGRESSION = '''    #[cfg(unix)]
    #[test]
    fn input_limits_rejects_symlinked_reserved_campaign_files() {
        use std::os::unix::fs::symlink;
        for name in ["campaign.json", "manifest.json"] {
            let directory = TempDirectory::new("reserved-symlink");
            let outside = TempDirectory::new("reserved-target");
            write_fixture(directory.path());
            let path = directory.path().join(name);
            let target = outside.path().join(name);
            fs::rename(&path, &target).unwrap();
            symlink(&target, &path).unwrap();
            assert!(verify_kv_campaign_directory(directory.path()).is_err(),
                "reserved symlink was accepted: {name}");
        }
    }

'''

if sys.argv[1] == 'regression':
    for name, expected in ORIGINALS.items():
        raw = (ROOT / name).read_bytes()
        actual = hashlib.sha1(f'blob {len(raw)}\0'.encode() + raw).hexdigest()
        if actual != expected:
            raise RuntimeError(f'original blob drift: {name}: {actual}')
    write('lib.rs', replace(read('lib.rs'), '    fn write_fixture(directory: &Path) {', REGRESSION + '    fn write_fixture(directory: &Path) {'))
    raise SystemExit(0)
if sys.argv[1] != 'apply':
    raise SystemExit('expected regression or apply')

source = read('lib.rs')
source = replace(source, '#![forbid(unsafe_code)]', '#![forbid(unsafe_code)]\n\npub mod input;\n\nuse input::{MAX_CAMPAIGN_ENTRIES, TextReadBudget};')
header = '''pub fn verify_kv_campaign_directory(
    directory: impl AsRef<Path>,
) -> Result<CampaignVerificationSummary, CampaignDirectoryError> {'''
new_header = '''/// Verify a persisted campaign using the default bounded file-read policy.
///
/// Accepted inputs are not normalized. This reads evidence; it executes no model.
pub fn verify_kv_campaign_directory(
    directory: impl AsRef<Path>,
) -> Result<CampaignVerificationSummary, CampaignDirectoryError> {
    verify_kv_campaign_directory_with_budget(directory, &mut TextReadBudget::default())
}

/// Verify one campaign while sharing an input-byte budget with other verifiers.
///
/// Manifests and evidence all consume the supplied budget, including repeated
/// reads. A directory contains at most `input::MAX_CAMPAIGN_ENTRIES` entries.
/// The caller must abort the composed operation on any error and keep inputs
/// trusted and unmodified. This is not an overall process-memory limit.
///
/// # Examples
///
/// ```no_run
/// use prospect_cli::{input::TextReadBudget, verify_kv_campaign_directory_with_budget};
/// let mut budget = TextReadBudget::default();
/// let first = verify_kv_campaign_directory_with_budget("campaign-a", &mut budget)?;
/// let second = verify_kv_campaign_directory_with_budget("campaign-b", &mut budget)?;
/// assert!(!first.policies().is_empty() && !second.policies().is_empty());
/// # Ok::<(), prospect_cli::CampaignDirectoryError>(())
/// ```
pub fn verify_kv_campaign_directory_with_budget(
    directory: impl AsRef<Path>,
    budget: &mut TextReadBudget,
) -> Result<CampaignVerificationSummary, CampaignDirectoryError> {'''
source = replace(source, header, new_header)
source = replace(source, 'read_text(&manifest_path)?', 'read_text(&manifest_path, budget)?')
source = replace(source, 'read_text(&campaign_path)?', 'read_text(&campaign_path, budget)?')
source = replace(source, '    for entry in entries {', '''    for (entry_index, entry) in entries.enumerate() {
        if entry_index >= MAX_CAMPAIGN_ENTRIES {
            return Err(CampaignDirectoryError::Io {
                path: directory.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidData,
                    format!("campaign entry limit exceeded ({MAX_CAMPAIGN_ENTRIES})")),
            });
        }''')
source = replace(source, 'read_text(&path)?', 'read_text(&path, budget)?')
source = replace(source, 'fn read_text(path: &Path) -> Result<String, CampaignDirectoryError> {\n    fs::read_to_string(path)', 'fn read_text(path: &Path, budget: &mut TextReadBudget) -> Result<String, CampaignDirectoryError> {\n    budget.read_text(path)')
MORE_TESTS = '''    #[test]
    fn input_limits_shared_budget_covers_manifests_and_evidence() {
        let directory = TempDirectory::new("shared-budget");
        write_fixture(directory.path());
        let total = ["campaign.json", "manifest.json", "selection-000.json"]
            .iter().map(|name| fs::read(directory.path().join(name)).unwrap().len()).sum::<usize>();
        let mut exact = super::TextReadBudget::new(total, total).unwrap();
        let summary = super::verify_kv_campaign_directory_with_budget(directory.path(), &mut exact).unwrap();
        assert_eq!(summary.record_count(), 1);
        assert_eq!(exact.remaining_bytes(), 0);
        let mut short = super::TextReadBudget::new(total, total - 1).unwrap();
        assert!(super::verify_kv_campaign_directory_with_budget(directory.path(), &mut short).is_err());
    }

    #[test]
    fn input_limits_rejects_too_many_directory_entries() {
        let directory = TempDirectory::new("entry-budget");
        write_fixture(directory.path());
        for index in 0..super::MAX_CAMPAIGN_ENTRIES {
            fs::write(directory.path().join(format!("extra-{index:04}.json")), "").unwrap();
        }
        let error = verify_kv_campaign_directory(directory.path()).unwrap_err();
        assert!(error.to_string().contains("entry limit exceeded"));
    }

'''
source = replace(source, '    fn write_fixture(directory: &Path) {', MORE_TESTS + '    fn write_fixture(directory: &Path) {')
write('lib.rs', source)

# Single-file entry points use the same read policy. Test imports stay local.
for name in ['kv_campaign_spec.rs', 'scenario_bundle.rs', 'dispatch_preflight.rs']:
    source = read(name)
    source = replace(source, 'use std::fs;\n', '', count=source.count('use std::fs;\n')) if False else source
    # Only remove the top-level fs import; existing test-module imports remain.
    source = replace(source, '\nuse std::fs;\n', '\n', count=1)
    if name == 'dispatch_preflight.rs':
        source = replace(source, '    let bundle_path = bundle_path.as_ref();', '    let mut budget = prospect_cli::input::TextReadBudget::default();\n    let bundle_path = bundle_path.as_ref();')
        source = source.replace('fs::read_to_string(bundle_path)', 'budget.read_text(bundle_path)')
        source = source.replace('fs::read_to_string(catalog_path)', 'budget.read_text(catalog_path)')
    else:
        source = replace(source, 'fs::read_to_string(path)', 'prospect_cli::input::read_text(path)')
    write(name, source)

source = read('kv_campaign_suite.rs')
source = replace(source, 'use prospect_cli::{CampaignVerificationSummary, verify_kv_campaign_directory};', '''use prospect_cli::{CampaignVerificationSummary, verify_kv_campaign_directory_with_budget};
use prospect_cli::input::TextReadBudget;
#[cfg(test)]
use prospect_cli::verify_kv_campaign_directory;''')
old = '''fn verify_suite_with_contract(
    directory: &Path,
    contract: &SuiteContract,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {'''
new = old + '''
    verify_suite_with_budget(directory, contract, &mut TextReadBudget::default())
}

fn verify_suite_with_budget(
    directory: &Path,
    contract: &SuiteContract,
    budget: &mut TextReadBudget,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {'''
source = replace(source, old, new)
source = replace(source, 'read_text(&manifest_path)?', 'read_text(&manifest_path, budget)?')
source = replace(source, 'verify_kv_campaign_directory(&campaign_directory)', 'verify_kv_campaign_directory_with_budget(&campaign_directory, budget)')
source = replace(source, 'verify_published_summary(directory, entry, &summary)?', 'verify_published_summary(directory, entry, &summary, budget)?')
source = replace(source, 'verify_campaign_context(&campaign_directory, &manifest, entry, contract)?', 'verify_campaign_context(&campaign_directory, &manifest, entry, contract, budget)?')
source = replace(source, 'load_observed_comparison(&campaign_directory, entry.retained_count)?', 'load_observed_comparison(&campaign_directory, entry.retained_count, budget)?')
source = replace(source, '''fn verify_published_summary(
    directory: &Path,
    entry: &SuiteCampaignWire,
    summary: &CampaignVerificationSummary,
)''', '''fn verify_published_summary(
    directory: &Path,
    entry: &SuiteCampaignWire,
    summary: &CampaignVerificationSummary,
    budget: &mut TextReadBudget,
)''')
source = replace(source, '''    entry: &SuiteCampaignWire,
    contract: &SuiteContract,
)''', '''    entry: &SuiteCampaignWire,
    contract: &SuiteContract,
    budget: &mut TextReadBudget,
)''')
source = replace(source, '''fn load_observed_comparison(
    campaign_directory: &Path,
    retained_count: usize,
)''', '''fn load_observed_comparison(
    campaign_directory: &Path,
    retained_count: usize,
    budget: &mut TextReadBudget,
)''')
source = replace(source, 'read_text(&campaign_directory.join("campaign.json"))?', 'read_text(&campaign_directory.join("campaign.json"), budget)?')
source = replace(source, 'read_text(&path)?', 'read_text(&path, budget)?', count=2)
source = replace(source, 'fn read_text(path: &Path) -> Result<String, KvCampaignSuiteError> {', 'fn read_text(path: &Path, budget: &mut TextReadBudget) -> Result<String, KvCampaignSuiteError> {')
source = replace(source, '    fs::read_to_string(path).map_err(|source| KvCampaignSuiteError::Io {', '    budget.read_text(path).map_err(|source| KvCampaignSuiteError::Io {')
# The fixed suite contains exactly two evidence files plus two campaign inputs;
# cap the preliminary file-type walk as well, before generic campaign loading.
anchor = 'fn validate_campaign_entry_types(directory: &Path) -> Result<(), KvCampaignSuiteError> {'
start = source.index(anchor)
end = source.index('\nfn read_text(', start)
piece = source[start:end]
piece = replace(piece, '    for entry in entries {', '''    for (index, entry) in entries.enumerate() {
        if index >= POLICIES.len() + 2 {
            return Err(KvCampaignSuiteError::InvalidManifest("too many campaign entries"));
        }''')
source = source[:start] + piece + source[end:]
SUITE_TEST = '''    #[test]
    fn input_limits_suite_shares_one_budget_across_all_reads() {
        for contract in [&R1_CONTRACT, &R2_CONTRACT] {
            let directory = TempDirectory::new("suite-read-budget");
            write_suite_for_contract(directory.path(), false, None, contract);
            let size = |path: PathBuf| fs::read(path).unwrap().len();
            let mut total = size(directory.path().join("suite-manifest.json"));
            for count in RETAIN_COUNTS {
                let campaign = directory.path().join(format!("retain-{count:02}-of-27"));
                total += size(campaign.join("manifest.json"));
                // Context and baseline comparison reread these exact inputs.
                total += 2 * size(campaign.join("campaign.json"));
                for index in 0..POLICIES.len() {
                    total += 2 * size(campaign.join(format!("selection-{index:03}.json")));
                }
                total += size(directory.path().join(format!("verification-retain-{count:02}-of-27.json")));
            }
            let mut exact = TextReadBudget::new(total, total).unwrap();
            assert!(verify_suite_with_budget(directory.path(), contract, &mut exact).is_ok());
            assert_eq!(exact.remaining_bytes(), 0);
            let mut short = TextReadBudget::new(total, total - 1).unwrap();
            assert!(verify_suite_with_budget(directory.path(), contract, &mut short).is_err());
        }
    }

'''
source = replace(source, '    fn write_suite(directory: &Path, baseline_drift: bool) {', SUITE_TEST + '    fn write_suite(directory: &Path, baseline_drift: bool) {')
write('kv_campaign_suite.rs', source)

# Fail if a production CLI loading path still bypasses bounded input.
for name in ORIGINALS:
    production = read(name).split('#[cfg(test)]\nmod tests {', 1)[0]
    if 'fs::read_to_string(' in production:
        raise RuntimeError(f'unbounded production read remains: {name}')
