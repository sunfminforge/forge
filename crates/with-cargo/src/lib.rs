//! with-cargo: what it decides, without touching the world. The arguments
//! it takes, the dirs it looks in for cargo and in which order, and the
//! PATH it gives the command it runs. `world` checks the dirs, prints and
//! execs.

pub mod world;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

pub const NAME: &str = "with-cargo";

/// Exit code of an argument with-cargo does not take.
pub const EXIT_USAGE: u8 = 2;
/// Exit code when no dir looked in holds cargo, or the command cannot be
/// started, as a shell answers a command it cannot find.
pub const EXIT_NOT_FOUND: u8 = 127;

pub const USAGE: &str = "usage: with-cargo <command> [args…] | with-cargo --dir";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: with-cargo <command> [args…]\n",
    "       with-cargo -- <command> [args…]\n",
    "       with-cargo --dir\n",
    "       with-cargo --version\n\n",
    "Runs <command> with its args as given, with the cargo bin dir put first on that command's PATH, so cargo, rustc and every tool cargo installed run from a shell whose PATH lacks ~/.cargo/bin. Example: with-cargo cargo test -p feed-parse. It execs the command in its own place: the command's stdin, stdout and stderr are with-cargo's, and with-cargo exits with the command's exit code, unchanged. Only that command's PATH changes; no shell profile and no caller's environment is touched.\n\n",
    "The cargo bin dir is the first of these that holds an executable file named cargo:\n",
    "  $CARGO_HOME/bin           when CARGO_HOME is set and not empty\n",
    "  $HOME/.cargo/bin          when HOME is set and not empty\n\n",
    "options:\n",
    "  --dir                     print the cargo bin dir it would put on PATH, one line on stdout, and exit 0\n",
    "  --                        end of with-cargo's options: what follows is the command, even when it starts with -\n",
    "  -h, --help                print this help\n",
    "  -V, --version             print with-cargo's version\n\n",
    "Past the command's name every argument is the command's, --help and --dir included.\n\n",
    "Exits with the command's exit code. Exits {EXIT_NOT_FOUND} with one line on stderr naming every dir it looked in when none holds cargo (fix: install Rust with rustup, or set CARGO_HOME to the dir whose bin holds cargo), or when the command cannot be started (fix: name a command on PATH or in the cargo bin dir); {EXIT_USAGE} with the usage on stderr on an argument it does not take.\n",
);

/// What `--help` answers, its exit codes as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_NOT_FOUND}", &EXIT_NOT_FOUND.to_string())
        .replace("{EXIT_USAGE}", &EXIT_USAGE.to_string())
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Dir,
    Run {
        command: OsString,
        args: Vec<OsString>,
    },
}

/// The arguments, after the program's name. Only the first is
/// with-cargo's; from the command's name on, every argument is the
/// command's.
pub fn parse(args: &[OsString]) -> Result<Asked, String> {
    let Some((first, rest)) = args.split_first() else {
        return Err("a command is required, or --dir".into());
    };
    let (command, args) = match first.to_str() {
        Some("-h" | "--help") => return Ok(Asked::Help),
        Some("-V" | "--version") if rest.is_empty() => return Ok(Asked::Version),
        Some("--dir") if rest.is_empty() => return Ok(Asked::Dir),
        Some("--dir") => {
            return Err(
                "--dir takes nothing after it; drop the rest, or drop --dir to run it".into(),
            )
        }
        Some("--") => match rest.split_first() {
            Some((c, a)) => (c, a),
            None => return Err("a command is required after --".into()),
        },
        Some(f) if f.starts_with('-') => {
            return Err(format!(
                "no such option: {f}; to run a command named {f}, put -- before it"
            ))
        }
        _ => (first, rest),
    };
    Ok(Asked::Run {
        command: command.clone(),
        args: args.to_vec(),
    })
}

/// The dirs looked in for cargo, in order: $CARGO_HOME/bin, then
/// $HOME/.cargo/bin, each only when its variable is set and not empty.
pub fn cargo_dirs(cargo_home: Option<&OsStr>, home: Option<&OsStr>) -> Vec<PathBuf> {
    fn set(v: Option<&OsStr>) -> Option<&Path> {
        v.filter(|v| !v.is_empty()).map(Path::new)
    }
    let mut out = Vec::new();
    if let Some(h) = set(cargo_home) {
        out.push(h.join("bin"));
    }
    if let Some(h) = set(home) {
        out.push(h.join(".cargo").join("bin"));
    }
    out
}

/// The refusal when no dir holds cargo: every dir looked in, and the fix.
pub fn not_found(looked: &[PathBuf]) -> String {
    let looked = if looked.is_empty() {
        "nowhere: neither CARGO_HOME nor HOME is set".to_string()
    } else {
        looked
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "no executable cargo in the cargo bin dir (looked in {looked}); install Rust with rustup, or set CARGO_HOME to the dir whose bin holds cargo"
    )
}

/// The PATH the command runs with: the cargo bin dir, then the PATH as
/// it was. A dir PATH cannot hold (one with a ':') is refused.
pub fn new_path(dir: &Path, old: Option<&OsStr>) -> Result<OsString, String> {
    let mut dirs = vec![dir.to_path_buf()];
    if let Some(old) = old.filter(|o| !o.is_empty()) {
        dirs.extend(std::env::split_paths(old));
    }
    std::env::join_paths(dirs).map_err(|e| {
        format!(
            "cannot put {} on PATH: {e}; set CARGO_HOME to a dir whose path has no ':'",
            dir.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn parse_takes_only_its_first_argument() {
        assert_eq!(parse(&os(&["--dir"])), Ok(Asked::Dir));
        assert_eq!(parse(&os(&["--help"])), Ok(Asked::Help));
        assert_eq!(parse(&os(&["-V"])), Ok(Asked::Version));
        assert_eq!(
            parse(&os(&["cargo", "--help", "--dir"])),
            Ok(Asked::Run {
                command: "cargo".into(),
                args: os(&["--help", "--dir"])
            })
        );
        assert_eq!(
            parse(&os(&["--", "-x", "y"])),
            Ok(Asked::Run {
                command: "-x".into(),
                args: os(&["y"])
            })
        );
        assert!(parse(&[]).is_err());
        assert!(parse(&os(&["--"])).is_err());
        assert!(parse(&os(&["--dir", "x"])).is_err());
        assert!(parse(&os(&["--nope"]))
            .unwrap_err()
            .contains("put -- before it"));
    }

    #[test]
    fn cargo_home_comes_before_home_and_empty_is_unset() {
        assert_eq!(
            cargo_dirs(Some("/c".as_ref()), Some("/h".as_ref())),
            vec![PathBuf::from("/c/bin"), PathBuf::from("/h/.cargo/bin")]
        );
        assert_eq!(
            cargo_dirs(Some("".as_ref()), Some("/h".as_ref())),
            vec![PathBuf::from("/h/.cargo/bin")]
        );
        assert!(cargo_dirs(None, None).is_empty());
        assert!(not_found(&[]).contains("neither CARGO_HOME nor HOME"));
    }

    #[test]
    fn the_dir_goes_first_on_path() {
        assert_eq!(
            new_path(Path::new("/c/bin"), Some("/usr/bin:/bin".as_ref())).unwrap(),
            OsString::from("/c/bin:/usr/bin:/bin")
        );
        assert_eq!(
            new_path(Path::new("/c/bin"), None).unwrap(),
            OsString::from("/c/bin")
        );
        assert!(new_path(Path::new("/a:b/bin"), None).is_err());
    }
}
