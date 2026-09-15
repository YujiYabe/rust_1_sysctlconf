use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use sysctl_conf::{LoadError, parse_file};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let path = config_path(env::args_os().skip(1))?;
    let settings = parse_file(&path).map_err(|error| describe_load_error(&path, error))?;

    println!("{}", serde_json::to_string_pretty(&settings)?);

    Ok(())
}

fn config_path(mut arguments: impl Iterator<Item = std::ffi::OsString>) -> io::Result<PathBuf> {
    let path = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("sysctl.conf"));

    if arguments.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: sysctl-conf [FILE]",
        ));
    }

    Ok(path)
}

fn describe_load_error(path: &std::path::Path, error: LoadError) -> io::Error {
    let message = match &error {
        LoadError::Io(source) if source.kind() == io::ErrorKind::NotFound => {
            format!("configuration file not found: {}", path.display())
        }
        _ => format!("{}: {error}", path.display()),
    };

    io::Error::other(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn defaults_to_sample_sysctl_conf() {
        let path = config_path(std::iter::empty()).unwrap();
        assert_eq!(path, PathBuf::from("sysctl.conf"));
    }

    #[test]
    fn accepts_a_custom_path() {
        let path = config_path([OsString::from("sample.conf")].into_iter()).unwrap();
        assert_eq!(path, PathBuf::from("sample.conf"));
    }

    #[test]
    fn rejects_extra_arguments() {
        let error =
            config_path([OsString::from("one.conf"), OsString::from("two.conf")].into_iter())
                .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
