use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prospect_bundle::{ScenarioBundle, ScenarioBundleError};
use prospect_dispatch::catalog::{CatalogPreflightError, DispatchCatalog, DispatchCatalogError};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DispatchPreflightSummary {
    bundle_sha256: String,
    catalog_sha256: String,
    bundle_id: String,
    adapter_id: String,
    required_adapter_major: u16,
    required_adapter_minor: u16,
    offered_adapter_major: u16,
    offered_adapter_minor: u16,
    upstream_component: Option<String>,
    upstream_revision: Option<String>,
    metric_id: Option<String>,
    required_metric_major: Option<u16>,
    required_metric_minor: Option<u16>,
    offered_metric_major: Option<u16>,
    offered_metric_minor: Option<u16>,
    policy_id: Option<String>,
    required_policy_major: Option<u16>,
    required_policy_minor: Option<u16>,
    offered_policy_major: Option<u16>,
    offered_policy_minor: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegistryResolutionSummary {
    id: Option<String>,
    required_major: Option<u16>,
    required_minor: Option<u16>,
    offered_major: Option<u16>,
    offered_minor: Option<u16>,
}

#[derive(Debug)]
pub enum DispatchPreflightFileError {
    BundleIo { path: PathBuf, source: io::Error },
    CatalogIo { path: PathBuf, source: io::Error },
    Bundle(ScenarioBundleError),
    Catalog(DispatchCatalogError),
    Preflight(CatalogPreflightError),
}

pub fn preflight_scenario_bundle_files(
    bundle_path: impl AsRef<Path>,
    catalog_path: impl AsRef<Path>,
) -> Result<DispatchPreflightSummary, DispatchPreflightFileError> {
    let bundle_path = bundle_path.as_ref();
    let catalog_path = catalog_path.as_ref();
    let bundle_payload =
        fs::read_to_string(bundle_path).map_err(|source| DispatchPreflightFileError::BundleIo {
            path: bundle_path.to_path_buf(),
            source,
        })?;
    let catalog_payload = fs::read_to_string(catalog_path).map_err(|source| {
        DispatchPreflightFileError::CatalogIo {
            path: catalog_path.to_path_buf(),
            source,
        }
    })?;

    let bundle = ScenarioBundle::<Value, Value>::from_canonical_json(&bundle_payload)
        .map_err(DispatchPreflightFileError::Bundle)?;
    let catalog = DispatchCatalog::from_canonical_json(&catalog_payload)
        .map_err(DispatchPreflightFileError::Catalog)?;
    let resolved = catalog
        .resolve_bundle(&bundle)
        .map_err(DispatchPreflightFileError::Preflight)?;

    let bundle_sha256 = bundle
        .sha256()
        .map_err(DispatchPreflightFileError::Bundle)?;
    let catalog_sha256 = format!("{:x}", Sha256::digest(catalog_payload.as_bytes()));
    let required_adapter = bundle.adapter();
    let offered_adapter = resolved.adapter();
    let required_adapter_version = required_adapter.contract_version();
    let offered_adapter_version = offered_adapter.contract_version();
    let (upstream_component, upstream_revision) = required_adapter
        .upstream()
        .map(|upstream| {
            (
                Some(upstream.component().as_str().to_owned()),
                Some(upstream.revision().to_owned()),
            )
        })
        .unwrap_or((None, None));
    let metric = registry_summary(bundle.metric(), resolved.metric());
    let policy = registry_summary(bundle.policy(), resolved.policy());

    Ok(DispatchPreflightSummary {
        bundle_sha256,
        catalog_sha256,
        bundle_id: bundle.bundle_id().as_str().to_owned(),
        adapter_id: required_adapter.adapter_id().as_str().to_owned(),
        required_adapter_major: required_adapter_version.major(),
        required_adapter_minor: required_adapter_version.minor(),
        offered_adapter_major: offered_adapter_version.major(),
        offered_adapter_minor: offered_adapter_version.minor(),
        upstream_component,
        upstream_revision,
        metric_id: metric.id,
        required_metric_major: metric.required_major,
        required_metric_minor: metric.required_minor,
        offered_metric_major: metric.offered_major,
        offered_metric_minor: metric.offered_minor,
        policy_id: policy.id,
        required_policy_major: policy.required_major,
        required_policy_minor: policy.required_minor,
        offered_policy_major: policy.offered_major,
        offered_policy_minor: policy.offered_minor,
    })
}

fn registry_summary(
    required: Option<&prospect_bundle::RegistryRequirement>,
    offered: Option<&prospect_dispatch::catalog::AvailableRegistryEntry>,
) -> RegistryResolutionSummary {
    match (required, offered) {
        (Some(required), Some(offered)) => RegistryResolutionSummary {
            id: Some(required.id().as_str().to_owned()),
            required_major: Some(required.version().major()),
            required_minor: Some(required.version().minor()),
            offered_major: Some(offered.version().major()),
            offered_minor: Some(offered.version().minor()),
        },
        (None, None) => RegistryResolutionSummary {
            id: None,
            required_major: None,
            required_minor: None,
            offered_major: None,
            offered_minor: None,
        },
        _ => unreachable!("successful dispatch preflight preserves optional requirement shape"),
    }
}

impl fmt::Display for DispatchPreflightFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BundleIo { path, source } => {
                write!(
                    formatter,
                    "failed to read bundle {}: {source}",
                    path.display()
                )
            }
            Self::CatalogIo { path, source } => {
                write!(
                    formatter,
                    "failed to read dispatch catalog {}: {source}",
                    path.display()
                )
            }
            Self::Bundle(error) => {
                write!(formatter, "scenario bundle verification failed: {error}")
            }
            Self::Catalog(error) => {
                write!(formatter, "dispatch catalog verification failed: {error}")
            }
            Self::Preflight(error) => write!(formatter, "dispatch preflight failed: {error}"),
        }
    }
}

impl std::error::Error for DispatchPreflightFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BundleIo { source, .. } | Self::CatalogIo { source, .. } => Some(source),
            Self::Bundle(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::Preflight(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prospect_dispatch::catalog::CatalogPreflightError;

    use super::{DispatchPreflightFileError, preflight_scenario_bundle_files};

    const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
    const VALID_BUNDLE: &str = "{\"adapter\":{\"adapter_id\":\"prospect.fixture\",\"contract_version\":{\"major\":1,\"minor\":0},\"upstream\":{\"component\":\"memorithm.fixture\",\"revision\":\"0123456789abcdef0123456789abcdef01234567\"}},\"bundle_id\":\"bundle.fixture\",\"metric\":{\"id\":\"metric.distance\",\"version\":{\"major\":1,\"minor\":0}},\"policy\":{\"id\":\"policy.prefer\",\"version\":{\"major\":1,\"minor\":0}},\"scenarios\":[{\"id\":\"alpha\",\"intervention\":{\"delta\":1}}],\"schema\":\"prospect.scenario-bundle/v1\",\"seed\":7,\"state\":{\"load\":10}}";
    const VALID_CATALOG: &str = "{\"schema\":\"prospect.dispatch-catalog/v1\",\"adapters\":[{\"adapter_id\":\"prospect.fixture\",\"contract_version\":{\"major\":1,\"minor\":2},\"upstream\":{\"component\":\"memorithm.fixture\",\"revision\":\"0123456789abcdef0123456789abcdef01234567\"}}],\"metrics\":[{\"id\":\"metric.distance\",\"version\":{\"major\":1,\"minor\":3}}],\"policies\":[{\"id\":\"policy.prefer\",\"version\":{\"major\":1,\"minor\":1}}]}";

    struct TempFile(PathBuf);

    impl TempFile {
        fn write(label: &str, payload: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "prospect-dispatch-{label}-{}-{nonce}.json",
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
    fn preflights_canonical_bundle_against_canonical_catalog() {
        let bundle = TempFile::write("bundle", VALID_BUNDLE);
        let catalog = TempFile::write("catalog", VALID_CATALOG);
        let summary = preflight_scenario_bundle_files(&bundle.0, &catalog.0).unwrap();
        assert_eq!(summary.bundle_id, "bundle.fixture");
        assert_eq!(summary.adapter_id, "prospect.fixture");
        assert_eq!(
            (
                summary.required_adapter_major,
                summary.required_adapter_minor
            ),
            (1, 0)
        );
        assert_eq!(
            (summary.offered_adapter_major, summary.offered_adapter_minor),
            (1, 2)
        );
        assert_eq!(summary.upstream_revision.as_deref(), Some(REVISION));
        assert_eq!(summary.metric_id.as_deref(), Some("metric.distance"));
        assert_eq!(summary.offered_metric_minor, Some(3));
        assert_eq!(summary.policy_id.as_deref(), Some("policy.prefer"));
        assert_eq!(summary.offered_policy_minor, Some(1));
        assert_eq!(summary.bundle_sha256.len(), 64);
        assert_eq!(summary.catalog_sha256.len(), 64);
    }

    #[test]
    fn fails_closed_when_catalog_cannot_satisfy_bundle_metric() {
        let bundle = TempFile::write("bundle-missing-metric", VALID_BUNDLE);
        let catalog_payload = VALID_CATALOG.replace(
            "{\"id\":\"metric.distance\",\"version\":{\"major\":1,\"minor\":3}}",
            "{\"id\":\"metric.other\",\"version\":{\"major\":1,\"minor\":3}}",
        );
        let catalog = TempFile::write("catalog-missing-metric", &catalog_payload);
        assert!(matches!(
            preflight_scenario_bundle_files(&bundle.0, &catalog.0),
            Err(DispatchPreflightFileError::Preflight(
                CatalogPreflightError::MissingMetric(id)
            )) if id == "metric.distance"
        ));
    }

    #[test]
    fn rejects_noncanonical_catalog_before_preflight() {
        let bundle = TempFile::write("bundle-noncanonical-catalog", VALID_BUNDLE);
        let catalog = TempFile::write("catalog-noncanonical", &format!(" {VALID_CATALOG}"));
        assert!(matches!(
            preflight_scenario_bundle_files(&bundle.0, &catalog.0),
            Err(DispatchPreflightFileError::Catalog(_))
        ));
    }
}
