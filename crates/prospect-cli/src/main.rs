#![forbid(unsafe_code)]

mod scenario_bundle;

use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

use prospect_adapter::built_in_adapter_catalog;
use prospect_cli::verify_kv_campaign_directory;
use scenario_bundle::verify_scenario_bundle_file;

const USAGE: &str = "Usage:\n  prospect list-adapters\n  prospect verify-kv-campaign <campaign-directory>\n  prospect verify-scenario-bundle <bundle.json>";

fn main() -> ExitCode {
    match run(env::args_os()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(CliError::Usage(message)) => {
            eprintln!("{message}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Verification(message)) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run<I>(arguments: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let Some(command) = arguments.next() else {
        return Err(CliError::Usage("missing command".to_owned()));
    };

    match command.to_str() {
        Some("list-adapters") => {
            require_no_arguments(&mut arguments, "list-adapters")?;
            let catalog = built_in_adapter_catalog()
                .map_err(|error| CliError::Verification(error.to_string()))?;
            serde_json::to_string(&catalog).map_err(|error| {
                CliError::Verification(format!("failed to encode adapter catalog: {error}"))
            })
        }
        Some("verify-kv-campaign") => {
            let directory =
                exactly_one_argument(&mut arguments, "verify-kv-campaign", "campaign directory")?;
            let summary = verify_kv_campaign_directory(directory)
                .map_err(|error| CliError::Verification(error.to_string()))?;
            serde_json::to_string(&summary).map_err(|error| {
                CliError::Verification(format!("failed to encode campaign summary: {error}"))
            })
        }
        Some("verify-scenario-bundle") => {
            let path =
                exactly_one_argument(&mut arguments, "verify-scenario-bundle", "bundle file")?;
            let summary = verify_scenario_bundle_file(path)
                .map_err(|error| CliError::Verification(error.to_string()))?;
            serde_json::to_string(&summary).map_err(|error| {
                CliError::Verification(format!("failed to encode bundle summary: {error}"))
            })
        }
        _ => Err(CliError::Usage(format!(
            "unknown command {:?}",
            command.to_string_lossy()
        ))),
    }
}

fn require_no_arguments<I>(arguments: &mut I, command: &str) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    if arguments.next().is_some() {
        return Err(CliError::Usage(format!(
            "{command} does not accept positional arguments"
        )));
    }
    Ok(())
}

fn exactly_one_argument<I>(
    arguments: &mut I,
    command: &str,
    argument_name: &str,
) -> Result<OsString, CliError>
where
    I: Iterator<Item = OsString>,
{
    let Some(value) = arguments.next() else {
        return Err(CliError::Usage(format!(
            "{command} requires exactly one {argument_name}"
        )));
    };
    if arguments.next().is_some() {
        return Err(CliError::Usage(format!(
            "{command} accepts exactly one {argument_name}"
        )));
    }
    Ok(value)
}

#[derive(Debug, PartialEq, Eq)]
enum CliError {
    Usage(String),
    Verification(String),
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{CliError, run};

    #[test]
    fn rejects_unknown_command() {
        let error = run([
            OsString::from("prospect"),
            OsString::from("unknown"),
            OsString::from("campaign"),
        ])
        .unwrap_err();
        assert!(matches!(error, CliError::Usage(message) if message.contains("unknown command")));
    }

    #[test]
    fn list_adapters_emits_deterministic_catalog() {
        let output = run([OsString::from("prospect"), OsString::from("list-adapters")]).unwrap();
        let catalog: serde_json::Value = serde_json::from_str(&output).unwrap();
        let ids: Vec<_> = catalog
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["adapter_id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec![
                "prospect.elastic",
                "prospect.flat_boolean_attention",
                "prospect.kv_eviction",
                "prospect.tdi",
            ]
        );
    }

    #[test]
    fn list_adapters_rejects_extra_arguments() {
        assert!(matches!(
            run([
                OsString::from("prospect"),
                OsString::from("list-adapters"),
                OsString::from("unexpected")
            ]),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn kv_campaign_requires_exactly_one_directory() {
        assert!(matches!(
            run([
                OsString::from("prospect"),
                OsString::from("verify-kv-campaign")
            ]),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            run([
                OsString::from("prospect"),
                OsString::from("verify-kv-campaign"),
                OsString::from("a"),
                OsString::from("b")
            ]),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn scenario_bundle_requires_exactly_one_file() {
        assert!(matches!(
            run([
                OsString::from("prospect"),
                OsString::from("verify-scenario-bundle")
            ]),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            run([
                OsString::from("prospect"),
                OsString::from("verify-scenario-bundle"),
                OsString::from("a"),
                OsString::from("b")
            ]),
            Err(CliError::Usage(_))
        ));
    }
}
