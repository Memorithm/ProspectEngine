use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prospect_bundle::{ScenarioBundle, ScenarioBundleError};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScenarioBundleVerificationSummary {
    sha256: String,
    bundle_id: String,
    adapter_id: String,
    adapter_contract_major: u16,
    adapter_contract_minor: u16,
    upstream_component: Option<String>,
    upstream_revision: Option<String>,
    seed: Option<u64>,
    scenario_ids: Vec<String>,
    metric_id: Option<String>,
    metric_major: Option<u16>,
    metric_minor: Option<u16>,
    policy_id: Option<String>,
    policy_major: Option<u16>,
    policy_minor: Option<u16>,
}

#[derive(Debug)]
pub enum ScenarioBundleFileError {
    Io { path: PathBuf, source: io::Error },
    Bundle(ScenarioBundleError),
}

pub fn verify_scenario_bundle_file(
    path: impl AsRef<Path>,
) -> Result<ScenarioBundleVerificationSummary, ScenarioBundleFileError> {
    let path = path.as_ref();
    let payload = fs::read_to_string(path).map_err(|source| ScenarioBundleFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bundle = ScenarioBundle::<Value, Value>::from_canonical_json(&payload)
        .map_err(ScenarioBundleFileError::Bundle)?;
    let sha256 = bundle.sha256().map_err(ScenarioBundleFileError::Bundle)?;
    let adapter_version = bundle.adapter().contract_version();
    let (upstream_component, upstream_revision) = bundle
        .adapter()
        .upstream()
        .map(|upstream| {
            (
                Some(upstream.component().as_str().to_owned()),
                Some(upstream.revision().to_owned()),
            )
        })
        .unwrap_or((None, None));
    let (metric_id, metric_major, metric_minor) = requirement_summary(bundle.metric());
    let (policy_id, policy_major, policy_minor) = requirement_summary(bundle.policy());

    Ok(ScenarioBundleVerificationSummary {
        sha256,
        bundle_id: bundle.bundle_id().as_str().to_owned(),
        adapter_id: bundle.adapter().adapter_id().as_str().to_owned(),
        adapter_contract_major: adapter_version.major(),
        adapter_contract_minor: adapter_version.minor(),
        upstream_component,
        upstream_revision,
        seed: bundle.seed(),
        scenario_ids: bundle
            .scenarios()
            .iter()
            .map(|scenario| scenario.id().as_str().to_owned())
            .collect(),
        metric_id,
        metric_major,
        metric_minor,
        policy_id,
        policy_major,
        policy_minor,
    })
}

fn requirement_summary(
    requirement: Option<&prospect_bundle::RegistryRequirement>,
) -> (Option<String>, Option<u16>, Option<u16>) {
    match requirement {
        Some(requirement) => (
            Some(requirement.id().as_str().to_owned()),
            Some(requirement.version().major()),
            Some(requirement.version().minor()),
        ),
        None => (None, None, None),
    }
}

impl fmt::Display for ScenarioBundleFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Bundle(error) => {
                write!(formatter, "scenario bundle verification failed: {error}")
            }
        }
    }
}

impl std::error::Error for ScenarioBundleFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Bundle(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prospect_bundle::ScenarioBundleError;

    use super::{ScenarioBundleFileError, verify_scenario_bundle_file};

    const VALID_BUNDLE: &str = "{\"adapter\":{\"adapter_id\":\"prospect.fixture\",\"contract_version\":{\"major\":1,\"minor\":0},\"upstream\":{\"component\":\"memorithm.fixture\",\"revision\":\"0123456789abcdef0123456789abcdef01234567\"}},\"bundle_id\":\"bundle.fixture\",\"metric\":{\"id\":\"metric.distance\",\"version\":{\"major\":1,\"minor\":0}},\"policy\":null,\"scenarios\":[{\"id\":\"alpha\",\"intervention\":{\"delta\":1}}],\"schema\":\"prospect.scenario-bundle/v1\",\"seed\":7,\"state\":{\"load\":10}}";

    struct TempFile(PathBuf);

    impl TempFile {
        fn write(label: &str, payload: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "prospect-bundle-{label}-{}-{nonce}.json",
                std::process::id()
            ));
            fs::write(&path, payload).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn verifies_generic_bundle_and_reports_bindings() {
        let file = TempFile::write("valid", VALID_BUNDLE);
        let summary = verify_scenario_bundle_file(&file.0).unwrap();
        assert_eq!(summary.bundle_id, "bundle.fixture");
        assert_eq!(summary.adapter_id, "prospect.fixture");
        assert_eq!(summary.adapter_contract_major, 1);
        assert_eq!(summary.adapter_contract_minor, 0);
        assert_eq!(summary.scenario_ids, ["alpha"]);
        assert_eq!(summary.metric_id.as_deref(), Some("metric.distance"));
        assert_eq!(summary.seed, Some(7));
        assert_eq!(summary.sha256.len(), 64);
    }

    #[test]
    fn rejects_noncanonical_bundle_file() {
        let file = TempFile::write("noncanonical", &format!(" {VALID_BUNDLE}"));
        assert!(matches!(
            verify_scenario_bundle_file(&file.0),
            Err(ScenarioBundleFileError::Bundle(
                ScenarioBundleError::NonCanonicalJson
            ))
        ));
    }
}
