//! repo-glance: what it decides, without touching the world. The
//! arguments it takes, the branch line read from git's porcelain v2
//! headers, and the '== git' section as printed. The paths after it are
//! peek's to print. `world` runs git and reads the paths.

pub mod world;

pub const NAME: &str = "repo-glance";

/// Exit code when the git state or any path could not be read, and of a
/// usage error (an option repo-glance does not take).
pub const EXIT_FAILED: u8 = 2;

/// How many commits the log shows.
pub const COMMITS: u32 = 5;

pub const USAGE: &str = "usage: repo-glance [path…]";

/// What `--help` answers, before its numbers are put in by `help`.
const HELP: &str = concat!(
    "usage: repo-glance [path…]\n",
    "       repo-glance -- <path>…\n",
    "       repo-glance --version\n\n",
    "Orients in a worktree in one call instead of git status, git log and cat: prints the git state of the repo the current directory is in, then each named path. Example: repo-glance task.md README.md Cargo.toml crates. It only reads: it never changes the worktree or the index (git runs with optional locks off).\n\n",
    "First a '== git' header, then:\n",
    "  branch <name> -> <upstream> (ahead A, behind B)   the current branch and its upstream; '(no upstream)' when it has none, '(upstream gone)' when the upstream no longer resolves, 'branch (detached at <hash>)' on a detached HEAD.\n",
    "  status:          then git status --short, one line per changed or untracked file, paths relative to the current directory; 'status: clean' when there is none.\n",
    "  last {COMMITS} commits:  then one line per commit, newest first, '<short hash> <subject>'; 'last commits: none yet' before the first commit.\n",
    "Outside a git repo the section is one line 'not a git repository: <directory>; to see git state, run repo-glance from inside a worktree', and the paths are still printed. When git itself fails the section is an 'error: ' line with its fix.\n\n",
    "Then each named path, in the order named, as peek prints it, after a blank line: a '== <path>' header, then a file's lines as '<line>: <text>', '(binary, N bytes)' or '(empty, 0 bytes)'; a directory's entries one per line, sorted, each directory with a trailing '/'; a missing or unreadable path one 'error: <what failed>; <the fix>' line, the others still printed. Line ranges are grep-around's (<file>:<from>-<to>), not repo-glance's.\n\n",
    "options:\n",
    "  --              end of repo-glance's options: what follows is paths, even when one starts with -\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print repo-glance's version\n\n",
    "Exits 0 when the git state (or that there is no repo) and every path were read. Exits {EXIT_FAILED} when git could not be run or failed, or any path was missing or unreadable, after printing all the rest. Exits {EXIT_FAILED} too, with the usage on stderr and nothing on stdout, when an option is one it does not take.\n",
);

/// What `--help` answers, its numbers as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_FAILED}", &EXIT_FAILED.to_string())
        .replace("{COMMITS}", &COMMITS.to_string())
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Glance(Vec<String>),
}

/// The arguments, after the program's name. Options come first; after
/// `--` or the first path, every argument is a path. No path is fine:
/// then only the git state is printed.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let rest = match args.first().map(String::as_str) {
        None => args,
        Some("-h" | "--help") => return Ok(Asked::Help),
        Some("-V" | "--version") if args.len() == 1 => return Ok(Asked::Version),
        Some("--") => &args[1..],
        Some(f) if f.starts_with('-') && f.len() > 1 => {
            return Err(format!(
                "no such option: {f}; to glance at a path named {f}, put -- before it"
            ))
        }
        Some(_) => args,
    };
    Ok(Asked::Glance(rest.to_vec()))
}

/// The branch, as git's `status --porcelain=v2 --branch` headers say it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Branch {
    /// The branch's name, or `None` on a detached HEAD.
    pub head: Option<String>,
    /// HEAD's commit, or `None` before the first commit.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    /// Ahead and behind the upstream, when it resolves.
    pub ab: Option<(u64, u64)>,
}

/// The branch from porcelain v2 output; lines other than its
/// `# branch.` headers are ignored.
pub fn branch(porcelain_v2: &str) -> Branch {
    let mut b = Branch::default();
    for line in porcelain_v2.lines() {
        let Some(h) = line.strip_prefix("# branch.") else {
            continue;
        };
        let Some((key, value)) = h.split_once(' ') else {
            continue;
        };
        match key {
            "oid" if value != "(initial)" => b.oid = Some(value.to_string()),
            "head" if value != "(detached)" => b.head = Some(value.to_string()),
            "upstream" => b.upstream = Some(value.to_string()),
            "ab" => {
                let mut it = value.split(' ');
                let a = it.next().and_then(|t| t.strip_prefix('+')?.parse().ok());
                let z = it.next().and_then(|t| t.strip_prefix('-')?.parse().ok());
                if let (Some(a), Some(z)) = (a, z) {
                    b.ab = Some((a, z));
                }
            }
            _ => {}
        }
    }
    b
}

/// The branch line, newline included.
pub fn branch_line(b: &Branch) -> String {
    let name = match (&b.head, &b.oid) {
        (Some(h), _) => h.clone(),
        (None, Some(oid)) => format!("(detached at {})", &oid[..oid.len().min(7)]),
        (None, None) => "(detached)".to_string(),
    };
    let up = match (&b.upstream, b.ab) {
        (None, _) => " (no upstream)".to_string(),
        (Some(u), Some((a, z))) => format!(" -> {u} (ahead {a}, behind {z})"),
        (Some(u), None) => format!(" -> {u} (upstream gone)"),
    };
    format!("branch {name}{up}\n")
}

/// What git said of the repo the current directory is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Git {
    /// Inside a repo: its porcelain v2 headers, `git status --short`,
    /// and `git log` one commit per line (empty before the first).
    Repo {
        porcelain_v2: String,
        short: String,
        log: String,
    },
    /// The current directory is in no git repo.
    NotRepo { dir: String },
    /// git could not be run at all.
    Missing,
    /// git ran and failed: which command, and what it said.
    Failed { command: String, stderr: String },
}

/// The '== git' section as printed, header included.
pub fn git_section(g: &Git) -> String {
    let mut out = String::from("== git\n");
    match g {
        Git::Repo {
            porcelain_v2,
            short,
            log,
        } => {
            out.push_str(&branch_line(&branch(porcelain_v2)));
            push_block(&mut out, "status:", "status: clean", short);
            push_block(
                &mut out,
                &format!("last {COMMITS} commits:"),
                "last commits: none yet",
                log,
            );
        }
        Git::NotRepo { dir } => out.push_str(&format!(
            "not a git repository: {dir}; to see git state, run {NAME} from inside a worktree\n"
        )),
        Git::Missing => out.push_str(
            "error: git could not be run; put git on PATH (git --version should answer)\n",
        ),
        Git::Failed { command, stderr } => {
            let said = stderr.lines().next().unwrap_or("").trim();
            out.push_str(&format!(
                "error: {command} failed: {said}; run {command} to see why, and fix what it names\n"
            ));
        }
    }
    out
}

/// Whether the git state was read (a repo, or the answer that there is
/// none).
pub fn git_read(g: &Git) -> bool {
    matches!(g, Git::Repo { .. } | Git::NotRepo { .. })
}

/// A titled block: the title and its lines, or the empty line alone.
fn push_block(out: &mut String, title: &str, empty: &str, body: &str) {
    if body.trim().is_empty() {
        out.push_str(empty);
        out.push('\n');
        return;
    }
    out.push_str(title);
    out.push('\n');
    for line in body.lines() {
        out.push_str(line);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn options_come_first_and_no_path_is_fine() {
        assert_eq!(parse(&[]), Ok(Asked::Glance(vec![])));
        assert_eq!(parse(&s(&["--help"])), Ok(Asked::Help));
        assert_eq!(parse(&s(&["-V"])), Ok(Asked::Version));
        assert_eq!(parse(&s(&["--"])), Ok(Asked::Glance(vec![])));
        assert!(parse(&s(&["-x"])).unwrap_err().contains("put -- before it"));
        assert_eq!(parse(&s(&["--", "-x"])), Ok(Asked::Glance(s(&["-x"]))));
        assert_eq!(parse(&s(&["a", "-V"])), Ok(Asked::Glance(s(&["a", "-V"]))));
    }

    #[test]
    fn the_branch_line_from_porcelain_v2_headers() {
        let up = "# branch.oid 0123456789abcdef\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n1 .M N... x\n";
        assert_eq!(
            branch_line(&branch(up)),
            "branch main -> origin/main (ahead 2, behind 1)\n"
        );
        let none = "# branch.oid (initial)\n# branch.head main\n";
        assert_eq!(branch_line(&branch(none)), "branch main (no upstream)\n");
        let gone = "# branch.oid abc\n# branch.head f\n# branch.upstream origin/f\n";
        assert_eq!(
            branch_line(&branch(gone)),
            "branch f -> origin/f (upstream gone)\n"
        );
        let det = "# branch.oid 0123456789abcdef\n# branch.head (detached)\n";
        assert_eq!(
            branch_line(&branch(det)),
            "branch (detached at 0123456) (no upstream)\n"
        );
    }

    #[test]
    fn the_git_section_as_printed() {
        let g = Git::Repo {
            porcelain_v2: "# branch.oid a\n# branch.head main\n".into(),
            short: "".into(),
            log: "".into(),
        };
        assert_eq!(
            git_section(&g),
            "== git\nbranch main (no upstream)\nstatus: clean\nlast commits: none yet\n"
        );
        let g = Git::Repo {
            porcelain_v2: "# branch.oid a\n# branch.head main\n".into(),
            short: " M a\n?? b\n".into(),
            log: "abc one\n".into(),
        };
        assert_eq!(
            git_section(&g),
            "== git\nbranch main (no upstream)\nstatus:\n M a\n?? b\nlast 5 commits:\nabc one\n"
        );
        let n = git_section(&Git::NotRepo { dir: "/t".into() });
        assert!(n.contains("not a git repository: /t; "), "{n}");
        assert!(git_section(&Git::Missing).contains("put git on PATH"));
        let f = git_section(&Git::Failed {
            command: "git log".into(),
            stderr: "fatal: bad\nmore".into(),
        });
        assert_eq!(
            f,
            "== git\nerror: git log failed: fatal: bad; run git log to see why, and fix what it names\n"
        );
        assert!(help().contains("Exits 2 when git"));
        assert!(help().contains("last 5 commits:"));
    }
}
