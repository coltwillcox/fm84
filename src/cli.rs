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
        /// Where to write the active panel's directory on quitting, for a
        /// shell function to cd to - `--choosedir`.
        choosedir: Option<PathBuf>,
    },
    Help,
    Version,
    /// Print `cd84` for this shell - `--shell-init`.
    ShellInit(Shell),
}

/// The shells `--shell-init` writes `cd84` for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// bash and zsh take the same function.
    Posix,
    Fish,
    /// PowerShell, Windows's own or the cross-platform pwsh.
    Pwsh,
}

impl Shell {
    fn named(name: &str) -> Option<Self> {
        match name {
            "bash" | "zsh" | "sh" => Some(Shell::Posix),
            "fish" => Some(Shell::Fish),
            "powershell" | "pwsh" => Some(Shell::Pwsh),
            _ => None,
        }
    }
}

/// Read the arguments after the program name. Taken as OsStrings, so a
/// directory whose name is not valid UTF-8 can still be given.
///
/// --help, --version and --shell-init win wherever they appear, as they do in
/// most tools: whatever else was typed, that is what was wanted, and --help
/// most of all. `--` ends the options, for a directory whose name starts with
/// a dash. An option's value comes as the next argument or after `=`.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut directories = Vec::new();
    let mut options_done = false;
    let mut wanted = None;
    let mut choosedir = None;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let is_option = !options_done && arg.as_encoded_bytes().starts_with(b"-") && arg != "-";
        if !is_option {
            directories.push(PathBuf::from(arg));
            continue;
        }
        // Split at the first '=', leaving the value as it was given: a file
        // name need not be UTF-8.
        let bytes = arg.as_encoded_bytes();
        let (name, inline) = match bytes.iter().position(|&byte| byte == b'=') {
            // SAFETY: both halves split an OsStr at an ASCII '=', which
            // as_encoded_bytes guarantees is a boundary of the encoding.
            Some(at) => unsafe { (std::ffi::OsStr::from_encoded_bytes_unchecked(&bytes[..at]), Some(std::ffi::OsStr::from_encoded_bytes_unchecked(&bytes[at + 1..]).to_os_string())) },
            None => (arg.as_os_str(), None),
        };
        let name = name.to_string_lossy().into_owned();
        let mut value = |what: &str| inline.clone().or_else(|| args.next()).filter(|value| !value.is_empty()).ok_or_else(|| format!("'{name}' needs {what}"));
        match name.as_str() {
            "--" if inline.is_none() => options_done = true,
            "-h" | "--help" if inline.is_none() => wanted = Some(Command::Help),
            "-V" | "--version" if inline.is_none() => wanted = wanted.or(Some(Command::Version)),
            "--choosedir" => choosedir = Some(PathBuf::from(value("a file to write the directory to")?)),
            "--shell-init" => {
                let shell = value("a shell: bash, zsh, fish or powershell")?;
                let shell = shell.to_str().and_then(Shell::named).ok_or_else(|| format!("no cd84 for '{}': bash, zsh, fish or powershell", shell.to_string_lossy()))?;
                if wanted != Some(Command::Help) {
                    wanted = Some(Command::ShellInit(shell));
                }
            }
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
        choosedir,
    })
}

/// `cd84`: fm84, and then the shell goes to the directory it was left in. A
/// program cannot change its shell's directory, so fm84 writes it to a file
/// with --choosedir and the function does the cd. Nothing is written when
/// fm84 does not quit as it should, or quits with Shift+F10, and the shell
/// then stays where it was.
pub fn shell_init(shell: Shell) -> &'static str {
    match shell {
        Shell::Posix => {
            r#"cd84() {
    local tmp dir code
    tmp="$(mktemp)" || return
    command fm84 --choosedir "$tmp" "$@"
    code=$?
    # The x keeps a newline at the end of the name, which $(...) would drop.
    dir="$(cat -- "$tmp"; printf x)"
    dir="${dir%x}"
    rm -f -- "$tmp"
    if [ -n "$dir" ] && [ -d "$dir" ] && [ "$dir" != "$PWD" ]; then
        cd -- "$dir" || return
    fi
    return "$code"
}"#
        }
        Shell::Fish => {
            r#"function cd84 --description 'fm84, then cd to the directory it was left in'
    set -l tmp (mktemp); or return
    command fm84 --choosedir $tmp $argv
    set -l code $status
    set -l dir (command cat -- $tmp | string collect -N)
    command rm -f -- $tmp
    if test -n "$dir"; and test -d "$dir"; and test "$dir" != "$PWD"
        cd $dir; or return
    end
    return $code
end"#
        }
        Shell::Pwsh => {
            r#"function cd84 {
    $tmp = New-TemporaryFile
    try {
        & fm84 --choosedir $tmp.FullName @args
        $dir = Get-Content -LiteralPath $tmp.FullName -Raw -Encoding UTF8
    } finally {
        Remove-Item -LiteralPath $tmp.FullName -ErrorAction SilentlyContinue
    }
    if ($dir -and (Test-Path -LiteralPath $dir -PathType Container)) {
        Set-Location -LiteralPath $dir
    }
}"#
        }
    }
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
      --choosedir <FILE>    On quitting, write the active panel's directory
                            to FILE, for a shell function to cd to;
                            Shift+F10 quits without writing it
      --shell-init <SHELL>  Print cd84, a function that does that, for bash,
                            zsh, fish or powershell, and exit
  -h, --help                Print this help and exit
  -V, --version             Print the version and exit

Without a directory, a panel opens where it was when fm84 last quit if
Remember directories is on (F11), and in the current directory otherwise.

To leave the shell where fm84 was left, add this to ~/.bashrc or ~/.zshrc
and start fm84 as cd84:
  eval \"$(fm84 --shell-init bash)\"
For fish, fm84 --shell-init fish | source; for PowerShell, put
fm84 --shell-init powershell | Out-String | Invoke-Expression in $PROFILE.

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

    /// The man page names the version it describes; a version bump that
    /// leaves it behind fails here rather than shipping a stale page.
    #[test]
    fn the_man_page_is_for_this_version() {
        let page = include_str!("../doc/fm84.1");
        let title = page.lines().find(|line| line.starts_with(".TH ")).unwrap();
        assert!(title.contains(&format!("\"{}\"", version())), "{title}");
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
        assert_eq!(parse_strs(&[]), Ok(Command::Run { left: None, right: None, choosedir: None }));
        assert_eq!(
            parse_strs(&["/tmp"]),
            Ok(Command::Run {
                left: Some("/tmp".into()),
                right: None,
                choosedir: None
            })
        );
        assert_eq!(
            parse_strs(&["/tmp", "/home"]),
            Ok(Command::Run {
                left: Some("/tmp".into()),
                right: Some("/home".into()),
                choosedir: None,
            })
        );
        assert!(parse_strs(&["a", "b", "c"]).unwrap_err().contains("at most two"));
    }

    #[test]
    fn unknown_options_are_refused_and_dashes_can_be_names() {
        assert_eq!(parse_strs(&["--verbose"]), Err("unknown option '--verbose'".to_string()));
        assert!(parse_strs(&["-x", "/tmp"]).is_err());
        assert_eq!(
            parse_strs(&["--", "-weird"]),
            Ok(Command::Run {
                left: Some("-weird".into()),
                right: None,
                choosedir: None
            })
        );
        assert_eq!(
            parse_strs(&["-"]),
            Ok(Command::Run {
                left: Some("-".into()),
                right: None,
                choosedir: None
            })
        );
    }

    #[test]
    fn choosedir_takes_a_file_either_way_and_needs_one() {
        let run = |choosedir: &str| {
            Ok(Command::Run {
                left: Some("/tmp".into()),
                right: None,
                choosedir: Some(choosedir.into()),
            })
        };
        assert_eq!(parse_strs(&["--choosedir", "/x/dir", "/tmp"]), run("/x/dir"));
        assert_eq!(parse_strs(&["/tmp", "--choosedir=/x/a=b"]), run("/x/a=b"));
        assert!(parse_strs(&["--choosedir"]).unwrap_err().contains("needs a file"));
        assert!(parse_strs(&["--choosedir="]).unwrap_err().contains("needs a file"));
        // A value is not taken where none belongs.
        assert!(parse_strs(&["--help=yes"]).is_err());
    }

    #[test]
    fn shell_init_names_a_shell_and_help_still_wins() {
        assert_eq!(parse_strs(&["--shell-init", "bash"]), Ok(Command::ShellInit(Shell::Posix)));
        assert_eq!(parse_strs(&["--shell-init=zsh"]), Ok(Command::ShellInit(Shell::Posix)));
        assert_eq!(parse_strs(&["--shell-init", "fish", "--version"]), Ok(Command::ShellInit(Shell::Fish)));
        assert_eq!(parse_strs(&["--shell-init", "pwsh"]), Ok(Command::ShellInit(Shell::Pwsh)));
        assert_eq!(parse_strs(&["-h", "--shell-init", "bash"]), Ok(Command::Help));
        assert!(parse_strs(&["--shell-init", "tcsh"]).unwrap_err().contains("no cd84 for 'tcsh'"));
        assert!(parse_strs(&["--shell-init"]).unwrap_err().contains("needs a shell"));
        for shell in [Shell::Posix, Shell::Fish, Shell::Pwsh] {
            assert!(shell_init(shell).contains("fm84 --choosedir"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_need_not_be_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(b"/tmp/caf\xe9".to_vec());
        assert_eq!(
            parse([raw.clone()]),
            Ok(Command::Run {
                left: Some(PathBuf::from(raw)),
                right: None,
                choosedir: None
            })
        );
    }
}
