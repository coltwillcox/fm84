//! The command line: `fm84 [OPTIONS] [LEFT [RIGHT]]`. Small enough to read by
//! hand rather than bring in a parser for.

use std::ffi::OsString;
use std::path::PathBuf;

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Start, with the panels in these directories where given.
    Run {
        left: Option<PathBuf>,
        right: Option<PathBuf>,
    },
    Help,
    Version,
}

/// Read the arguments after the program name. Taken as OsStrings, so a
/// directory whose name is not valid UTF-8 can still be given.
///
/// --help and --version win wherever they appear, as they do in most tools:
/// whatever else was typed, that is what was wanted. `--` ends the options,
/// for a directory whose name starts with a dash.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut directories = Vec::new();
    let mut options_done = false;
    let mut wanted = None;

    for arg in args {
        let is_option = !options_done && arg.as_encoded_bytes().starts_with(b"-") && arg != "-";
        if !is_option {
            directories.push(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("--") => options_done = true,
            Some("-h" | "--help") => wanted = Some(Command::Help),
            Some("-V" | "--version") => wanted = wanted.or(Some(Command::Version)),
            _ => return Err(format!("unknown option '{}'", arg.to_string_lossy())),
        }
    }

    if let Some(command) = wanted {
        return Ok(command);
    }
    if directories.len() > 2 {
        return Err(format!("expected at most two directories, got {}", directories.len()));
    }
    let mut directories = directories.into_iter();
    Ok(Command::Run {
        left: directories.next(),
        right: directories.next(),
    })
}

pub fn version() -> String {
    format!("fm84 {}", env!("CARGO_PKG_VERSION"))
}

pub fn help() -> String {
    let config = crate::options::config_path("config").map_or_else(|| "none found".to_string(), |path| path.display().to_string());
    format!(
        "{version} - a synthwave dual-pane TUI file manager

Usage: fm84 [OPTIONS] [LEFT [RIGHT]]

Arguments:
  [LEFT]   Directory for the left panel
  [RIGHT]  Directory for the right panel

Options:
  -h, --help     Print this help and exit
  -V, --version  Print the version and exit

Without a directory, a panel opens where it was when fm84 last quit if
Remember directories is on (F11), and in the current directory otherwise.

Config: {config}
Press F1 inside for the keys.",
        version = version()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn help_and_version_win_wherever_they_are() {
        assert_eq!(parse_strs(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_strs(&["-h"]), Ok(Command::Help));
        assert_eq!(parse_strs(&["--version"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["-V"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["/tmp", "--version"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["--version", "--help"]), Ok(Command::Help));
        assert_eq!(parse_strs(&["a", "b", "c", "-h"]), Ok(Command::Help));
    }

    #[test]
    fn directories_go_left_then_right() {
        assert_eq!(parse_strs(&[]), Ok(Command::Run { left: None, right: None }));
        assert_eq!(parse_strs(&["/tmp"]), Ok(Command::Run { left: Some("/tmp".into()), right: None }));
        assert_eq!(
            parse_strs(&["/tmp", "/home"]),
            Ok(Command::Run {
                left: Some("/tmp".into()),
                right: Some("/home".into())
            })
        );
        assert!(parse_strs(&["a", "b", "c"]).unwrap_err().contains("at most two"));
    }

    #[test]
    fn unknown_options_are_refused_and_dashes_can_be_names() {
        assert_eq!(parse_strs(&["--verbose"]), Err("unknown option '--verbose'".to_string()));
        assert!(parse_strs(&["-x", "/tmp"]).is_err());
        assert_eq!(parse_strs(&["--", "-weird"]), Ok(Command::Run { left: Some("-weird".into()), right: None }));
        assert_eq!(parse_strs(&["-"]), Ok(Command::Run { left: Some("-".into()), right: None }));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_need_not_be_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(b"/tmp/caf\xe9".to_vec());
        assert_eq!(parse([raw.clone()]), Ok(Command::Run { left: Some(PathBuf::from(raw)), right: None }));
    }
}
