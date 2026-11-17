//! peek's binary run on fixture trees written to a temp dir of the
//! test's own. Each test prints what the binary printed, seen with
//! `-- --nocapture`. Every expected output is worked by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_peek");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory of this test run's own.
fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("peek")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// peek run in `dir`, so each argument is a name relative to it.
fn peek(dir: &Path, args: &[&str]) -> Output {
    let o = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    println!(
        "--- stdout:\n{}--- stderr:\n{}--- exit: {:?}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
        o.status.code()
    );
    o
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

/// A crate-like tree: Cargo.toml, README.md (no newline at its end),
/// src/ with lib.rs, tests/ empty, and .hidden.
fn tree(name: &str) -> PathBuf {
    let d = scratch(name);
    fs::create_dir_all(d.join("krate/src")).unwrap();
    fs::create_dir_all(d.join("krate/tests")).unwrap();
    fs::write(d.join("krate/Cargo.toml"), "[package]\nname = \"k\"\n").unwrap();
    fs::write(d.join("krate/README.md"), "# k").unwrap();
    fs::write(d.join("krate/.hidden"), "").unwrap();
    fs::write(d.join("krate/src/lib.rs"), "pub fn f() {}\n").unwrap();
    d
}

#[test]
fn files_and_directories_mixed_print_in_the_order_given() {
    let d = tree("mixed");
    let o = peek(
        &d,
        &["krate/Cargo.toml", "krate", "krate/src/lib.rs", "krate/src"],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== krate/Cargo.toml\n\
         1: [package]\n\
         2: name = \"k\"\n\
         \n\
         == krate\n\
         .hidden\n\
         Cargo.toml\n\
         README.md\n\
         src/\n\
         tests/\n\
         \n\
         == krate/src/lib.rs\n\
         1: pub fn f() {}\n\
         \n\
         == krate/src\n\
         lib.rs\n"
    );
    assert_eq!(err(&o), "");
}

#[test]
fn a_last_line_without_a_newline_gets_one_and_numbers_count_from_1() {
    let d = scratch("lines");
    let ten: String = (1..=10).map(|i| format!("l{i}\n")).collect();
    fs::write(d.join("ten.txt"), ten).unwrap();
    fs::write(d.join("short"), "a\n\nc").unwrap();
    let o = peek(&d, &["short", "ten.txt"]);
    assert_eq!(o.status.code(), Some(0));
    let want = "== short\n1: a\n2: \n3: c\n\n== ten.txt\n1: l1\n2: l2\n3: l3\n4: l4\n5: l5\n6: l6\n7: l7\n8: l8\n9: l9\n10: l10\n";
    assert_eq!(out(&o), want);
}

#[test]
fn a_trailing_slash_marks_each_directory_and_a_link_to_one() {
    let d = tree("slash");
    #[cfg(unix)]
    std::os::unix::fs::symlink("src", d.join("krate/to-src")).unwrap();
    let o = peek(&d, &["krate/"]);
    assert_eq!(o.status.code(), Some(0));
    let mut want = String::from("== krate/\n.hidden\nCargo.toml\nREADME.md\nsrc/\ntests/\n");
    if cfg!(unix) {
        want.push_str("to-src/\n");
    }
    assert_eq!(out(&o), want);
}

#[test]
fn an_empty_file_and_an_empty_directory_say_so() {
    let d = tree("empty");
    let o = peek(&d, &["krate/.hidden", "krate/tests"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== krate/.hidden\n(empty, 0 bytes)\n\n== krate/tests\n(empty directory)\n"
    );
}

#[test]
fn a_binary_file_prints_its_size_instead_of_its_content() {
    let d = tree("binary");
    fs::write(
        d.join("blob.bin"),
        [0x89, b'P', b'N', b'G', 0, 1, 2, b'\n', 3],
    )
    .unwrap();
    let o = peek(&d, &["blob.bin", "krate/README.md"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== blob.bin\n(binary, 9 bytes)\n\n== krate/README.md\n1: # k\n"
    );
}

#[test]
fn a_missing_path_prints_an_error_under_its_header_the_others_print_exit_2() {
    let d = tree("missing");
    let o = peek(&d, &["krate/README.md", "nope.txt", "krate/src"]);
    assert_eq!(o.status.code(), Some(2));
    let s = out(&o);
    assert!(
        s.starts_with(
            "== krate/README.md\n1: # k\n\n== nope.txt\nerror: no such file or directory; "
        ),
        "{s}"
    );
    assert!(s.ends_with("\n\n== krate/src\nlib.rs\n"), "{s}");
    let error = s.lines().find(|l| l.starts_with("error: ")).unwrap();
    assert!(
        error.contains("check the name with peek ."),
        "the fix names how to find the path: {error}"
    );
    assert_eq!(err(&o), "");
}

/// A dir holding ten.txt, lines 'l1' to 'l10'.
fn ten(name: &str) -> PathBuf {
    let d = tree(name);
    let ten: String = (1..=10).map(|i| format!("l{i}\n")).collect();
    fs::write(d.join("ten.txt"), ten).unwrap();
    d
}

#[test]
fn a_range_prints_only_those_lines_with_their_real_numbers() {
    let d = ten("range");
    let o = peek(&d, &["ten.txt:3-5", "ten.txt:10-10"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== ten.txt:3-5\n3: l3\n4: l4\n5: l5\n\n== ten.txt:10-10\n10: l10\n"
    );
    assert_eq!(err(&o), "");
}

#[test]
fn a_range_past_the_end_prints_the_lines_there_then_the_range_to_ask_exit_2() {
    let d = ten("range-past");
    let o = peek(&d, &["ten.txt:9-14", "krate/README.md"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        out(&o),
        "== ten.txt:9-14\n9: l9\n10: l10\n\
         error: lines 11-14 are past the end, ten.txt has 10 lines; ask peek ten.txt:9-10\n\
         \n== krate/README.md\n1: # k\n"
    );
}

#[test]
fn each_bad_range_prints_only_its_error_and_fix_exit_2() {
    let d = ten("range-bad");
    for (arg, want) in [
        (
            "ten.txt:11-12",
            "error: range 11-12 starts past the end, ten.txt has 10 lines; ask peek ten.txt:1-10, or peek ten.txt whole\n",
        ),
        (
            "ten.txt:0-3",
            "error: lines count from 1, not 0; ask peek ten.txt:1-3\n",
        ),
        (
            "ten.txt:5-2",
            "error: range starts at 5, after its end 2; ask peek ten.txt:2-5\n",
        ),
        (
            "krate/.hidden:1-1",
            "error: range 1-1 starts past the end, krate/.hidden is empty; there is no line to ask; peek krate/.hidden shows it\n",
        ),
    ] {
        let o = peek(&d, &[arg, "ten.txt:1-1"]);
        assert_eq!(o.status.code(), Some(2), "{arg}");
        assert_eq!(
            out(&o),
            format!("== {arg}\n{want}\n== ten.txt:1-1\n1: l1\n"),
            "{arg}"
        );
    }
    let o = peek(&d, &["nope.txt:1-2"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        out(&o).starts_with(
            "== nope.txt:1-2\nerror: no such file or directory; check the name with peek ."
        ),
        "{}",
        out(&o)
    );
}

#[test]
fn a_range_on_a_directory_prints_its_error_and_fix_exit_2() {
    let d = ten("range-dir");
    let o = peek(&d, &["krate:1-3"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        out(&o),
        "== krate:1-3\nerror: krate is a directory, and a line range is a file's; ask peek krate for its entries\n"
    );
}

#[test]
fn a_binary_file_with_a_range_prints_its_size() {
    let d = scratch("range-binary");
    fs::write(d.join("blob.bin"), [b'a', 0, b'\n', b'b']).unwrap();
    let o = peek(&d, &["blob.bin:1-1"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "== blob.bin:1-1\n(binary, 4 bytes)\n");
}

#[test]
fn a_file_named_like_a_range_is_read_as_that_file() {
    let d = ten("range-named");
    fs::write(d.join("x.txt:1-2"), "named\n").unwrap();
    fs::write(d.join("x.txt"), "a\nb\nc\n").unwrap();
    let o = peek(&d, &["x.txt:1-2", "x.txt:2-3"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== x.txt:1-2\n1: named\n\n== x.txt:2-3\n2: b\n3: c\n"
    );
}

#[test]
fn files_ranges_and_a_directory_mixed_in_one_call_range() {
    let d = ten("range-mixed");
    let o = peek(
        &d,
        &[
            "krate/Cargo.toml",
            "ten.txt:2-3",
            "krate/src",
            "krate/Cargo.toml:2-2",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== krate/Cargo.toml\n1: [package]\n2: name = \"k\"\n\
         \n== ten.txt:2-3\n2: l2\n3: l3\n\
         \n== krate/src\nlib.rs\n\
         \n== krate/Cargo.toml:2-2\n2: name = \"k\"\n"
    );
    assert_eq!(err(&o), "");
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_prints_an_error_and_exits_2() {
    use std::os::unix::fs::PermissionsExt;
    let d = tree("unreadable");
    let f = d.join("secret");
    fs::write(&f, "x\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&f).is_ok() {
        // Running as root reads it anyway; there is nothing to refuse.
        return;
    }
    let o = peek(&d, &["secret", "krate/README.md"]);
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(o.status.code(), Some(2));
    let s = out(&o);
    assert!(
        s.starts_with("== secret\nerror: permission denied; "),
        "{s}"
    );
    assert!(s.ends_with("== krate/README.md\n1: # k\n"), "{s}");
}

#[test]
fn a_path_after_dash_dash_may_start_with_a_dash() {
    let d = scratch("dash");
    fs::write(d.join("-n"), "x\n").unwrap();
    let o = peek(&d, &["--", "-n"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "== -n\n1: x\n");
}

#[test]
fn no_path_or_an_unknown_option_is_refused_with_its_fix_exit_2() {
    let d = scratch("usage");
    let o = peek(&d, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(out(&o), "");
    assert!(err(&o).contains("name at least one path"), "{}", err(&o));
    let o = peek(&d, &["-x"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(out(&o), "");
    assert!(err(&o).contains("put -- before it"), "{}", err(&o));
}

#[test]
fn help_names_what_it_prints_and_its_exit_codes_version_its_number() {
    let d = scratch("help");
    let o = peek(&d, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    let h = out(&o);
    for needle in [
        "usage: peek <path>[:<from>-<to>]…",
        "== <argument>",
        "src/lib.rs:10-20",
        "(binary, N bytes)",
        "trailing '/'",
        "<path>:<from>-<to>",
        "Exits 0 when every argument was read",
        "Exits 2 when any argument failed",
    ] {
        assert!(h.contains(needle), "help lacks {needle:?}:\n{h}");
    }
    let o = peek(&d, &["--version"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), format!("peek {}\n", env!("CARGO_PKG_VERSION")));
}
