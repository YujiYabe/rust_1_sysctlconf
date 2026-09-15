//! Parser for files using the Linux `sysctl.conf` syntax.
//!
//! Later definitions of the same key overwrite earlier definitions, matching
//! the behavior users normally expect when loading a configuration file.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor};
use std::path::Path;

use serde_json::{Map, Value};

/// Parsed sysctl settings represented as a nested map.
///
/// Dots in a key delimit nested objects. For example, `log.file` is stored as
/// `{ "log": { "file": "..." } }`.
pub type SysctlMap = Map<String, Value>;

/// An error found in a sysctl configuration line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    line: usize,
    content: String,
}

impl ParseError {
    /// One-based line number on which parsing failed.
    pub fn line(&self) -> usize {
        self.line
    }

    /// The invalid line, without its line terminator.
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid sysctl configuration at line {}: {:?}",
            self.line, self.content
        )
    }
}

impl Error for ParseError {}

/// An error returned while reading and parsing a configuration.
#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    Parse(ParseError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read sysctl configuration: {error}"),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

impl From<io::Error> for LoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ParseError> for LoadError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

/// Parses a UTF-8 string containing sysctl configuration.
pub fn parse_str(input: &str) -> Result<SysctlMap, ParseError> {
    // Cursor implements BufRead and cannot produce an I/O error.
    match parse_reader(Cursor::new(input.as_bytes())) {
        Ok(settings) => Ok(settings),
        Err(LoadError::Parse(error)) => Err(error),
        Err(LoadError::Io(error)) => unreachable!("reading from a string failed: {error}"),
    }
}

/// Parses sysctl configuration from a buffered reader.
pub fn parse_reader<R: BufRead>(reader: R) -> Result<SysctlMap, LoadError> {
    let mut settings = Map::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if let Some((key, value)) = parse_line(&line, index + 1)? {
            insert_nested(&mut settings, key, value.to_owned());
        }
    }

    Ok(settings)
}

fn insert_nested(settings: &mut SysctlMap, key: &str, value: String) {
    let parts: Vec<_> = key.split('.').collect();
    insert_path(settings, &parts, value);
}

fn insert_path(settings: &mut SysctlMap, parts: &[&str], value: String) {
    if let [key] = parts {
        settings.insert((*key).to_owned(), Value::String(value));
        return;
    }

    let object = settings
        .entry(parts[0].to_owned())
        .or_insert_with(|| Value::Object(Map::new()));

    // If an earlier setting used this path as a scalar value, the later
    // nested setting replaces it, preserving the usual last-value behavior.
    if !object.is_object() {
        *object = Value::Object(Map::new());
    }

    insert_path(object.as_object_mut().unwrap(), &parts[1..], value);
}

/// Opens and parses a sysctl configuration file at an arbitrary path.
pub fn parse_file(path: impl AsRef<Path>) -> Result<SysctlMap, LoadError> {
    let file = File::open(path)?;
    parse_reader(BufReader::new(file))
}

/// Alias for [`parse_file`], for callers that prefer loading terminology.
pub fn load_file(path: impl AsRef<Path>) -> Result<SysctlMap, LoadError> {
    parse_file(path)
}

fn parse_line(line: &str, line_number: usize) -> Result<Option<(&str, &str)>, ParseError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        return Ok(None);
    }

    // A leading '-' is accepted by sysctl.d to suppress errors while applying
    // a setting. It is execution metadata and is not part of the key.
    let line = line.strip_prefix('-').unwrap_or(line).trim_start();
    let pair = if let Some((key, value)) = line.split_once('=') {
        Some((key.trim(), value.trim()))
    } else {
        line.find(char::is_whitespace)
            .map(|separator| (&line[..separator], line[separator..].trim()))
    };

    match pair {
        Some((key, value)) if !key.is_empty() && !value.is_empty() => Ok(Some((key, value))),
        _ => Err(ParseError {
            line: line_number,
            content: line.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn parses_normal_sysctl_syntax() {
        let input = r#"
            # kernel settings
            ; another comment
            net.ipv4.ip_forward = 1
            vm.swappiness 10
            kernel.domainname = example.local
        "#;

        let parsed = parse_str(input).unwrap();

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed["net"]["ipv4"]["ip_forward"], "1");
        assert_eq!(parsed["vm"]["swappiness"], "10");
        assert_eq!(parsed["kernel"]["domainname"], "example.local");
    }

    #[test]
    fn later_values_overwrite_earlier_values() {
        let parsed = parse_str("vm.swappiness = 60\nvm.swappiness = 5\n").unwrap();
        assert_eq!(parsed["vm"]["swappiness"], "5");
    }

    #[test]
    fn accepts_error_suppression_prefix() {
        let parsed = parse_str("-net.ipv6.conf.all.disable_ipv6 = 1\n").unwrap();
        assert_eq!(parsed["net"]["ipv6"]["conf"]["all"]["disable_ipv6"], "1");
    }

    #[test]
    fn preserves_spaces_and_comment_characters_in_values() {
        let parsed = parse_str("kernel.domainname = example # value\n").unwrap();
        assert_eq!(parsed["kernel"]["domainname"], "example # value");
    }

    #[test]
    fn dotted_keys_create_nested_maps() {
        let parsed = parse_str(
            "endpoint = localhost:3000\nlog.file = /var/log/console.log\nlog.name = default.log\n",
        )
        .unwrap();

        assert_eq!(parsed["endpoint"], "localhost:3000");
        assert_eq!(parsed["log"]["file"], "/var/log/console.log");
        assert_eq!(parsed["log"]["name"], "default.log");
        assert!(parsed.get("log.file").is_none());
    }

    #[test]
    fn reports_invalid_line_number_and_content() {
        let error = parse_str("ok.key = value\ninvalid\n").unwrap_err();
        assert_eq!(error.line(), 2);
        assert_eq!(error.content(), "invalid");
    }

    #[test]
    fn propagates_reader_errors() {
        let reader = io::BufReader::new(FailingReader);
        assert!(matches!(parse_reader(reader), Err(LoadError::Io(_))));
    }

    struct FailingReader;

    impl io::Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("test error"))
        }
    }
}
