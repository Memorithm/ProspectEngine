#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

use prospect_cli::verify_kv_campaign_directory;

const USAGE: &str = "Usage: prospect verify-kv-campaign <campaign-directory>";

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
    if command != "verify-kv-campaign" {
        return Err(CliError::Usage(format!(
            "unknown command {:?}",
            command.to_string_lossy()
        )));
    }
    let Some(directory) = arguments.next() else {
        return Err(CliError::Usage(
            "verify-kv-campaign requires a campaign directory".to_owned(),
        ));
    };
    if arguments.next().is_some() {
        return Err(CliError::Usage(
            "verify-kv-campaign accepts exactly one campaign directory".to_owned(),
        ));
    }

    let summary = verify_kv_campaign_directory(directory)
        .map_err(|error| CliError::Verification(error.to_string()))?;
    serde_json::to_string(&summary)
        .map_err(|error| CliError::Verification(format!("failed to encode summary: {error}")))
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
    fn requires_exactly_one_campaign_directory() {
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
}
