#![warn(rust_2018_idioms)]

use camino::Utf8Path as Path;
use cargo_metadata::MetadataCommand;
use difference::assert_diff;
use duct::cmd;
use std::{
    env, fs, io,
    str::{self, Utf8Error},
};
use tempdir::TempDir;
use termcolor::NoColor;

#[test]
fn normal() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-include-normal")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");

    fs::write(
        tempdir_path.join("Cargo.toml"),
        r#"[workspace]
members = ["a"]
exclude = ["b"]
[workspace.dependencies]
bitflags = "2.6.0"
rand = "0.8"
"#,
    )?;
    cargo_new(&tempdir_path.join("a"))?;
    cargo_new(&tempdir_path.join("b"))?;
    cargo_add_dep(&tempdir_path.join("b"), "b", "rand@0.8")?;
    cargo_add_dep(&tempdir_path.join("b"), "b", "bitflags@~2.5")?;

    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("b").join("Cargo.toml")).unwrap());

    let mut stderr = vec![];

    cargo_member::Include::new(tempdir_path, [tempdir_path.join("b")])
        .force(false)
        .dry_run(false)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("Cargo.toml")).unwrap());
    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("b").join("Cargo.toml")).unwrap());
    insta::with_settings!({filters => vec![
        (r"/tmp/.*/Cargo.lock", "<Cargo.lock>"),
    ]}, {
        insta::assert_snapshot!(String::from_utf8_lossy(&stderr));
    });
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &["--locked"])?;

    Ok(())
}

#[test]
fn force_nonexisting() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-include-force-nonexisting")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");
    fs::write(tempdir_path.join("Cargo.toml"), ORIGINAL)?;

    let mut stderr = vec![];

    cargo_member::Include::new(tempdir_path, [tempdir_path.join("nonexisting")])
        .force(true)
        .dry_run(false)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    assert_manifest(&tempdir_path.join("Cargo.toml"), EXPECTED_MANIFEST)?;
    assert_stderr(&stderr, EXPECTED_STDERR)?;
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &[]).unwrap_err();
    return Ok(());

    static ORIGINAL: &str = r#"[workspace]
members = []
exclude = []
"#;

    static EXPECTED_MANIFEST: &str = r#"[workspace]
members = ["nonexisting"]
exclude = []
"#;

    static EXPECTED_STDERR: &str = r#"      Adding "nonexisting" to `workspace.members`
"#;
}

#[test]
fn dry_run() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-include-dry-run")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");

    fs::write(tempdir_path.join("Cargo.toml"), MANIFEST)?;
    cargo_new(&tempdir_path.join("a"))?;
    cargo_new(&tempdir_path.join("b"))?;
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &[])?;
    cargo_add_dep(&tempdir_path.join("b"), "b", "rand@0.8")?;

    insta::assert_snapshot!(
        "b/Cargo.toml before",
        fs::read_to_string(tempdir_path.join("b").join("Cargo.toml")).unwrap()
    );

    let mut stderr = vec![];

    cargo_member::Include::new(tempdir_path, [tempdir_path.join("b")])
        .force(false)
        .dry_run(true)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    assert_manifest(&tempdir_path.join("Cargo.toml"), MANIFEST)?;
    insta::with_settings!({filters => vec![
        (r"/tmp/.*/Cargo.lock", "<Cargo.lock>"),
    ]}, {
        insta::assert_snapshot!(String::from_utf8_lossy(&stderr));
    });
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &["--locked"])?;
    return Ok(());

    static MANIFEST: &str = r#"[workspace]
members = ["a"]
exclude = ["b"]
[workspace.dependencies]
bitflags = "2.6"
rand = "0.8"
"#;
}

fn cargo_new(path: &Path) -> io::Result<()> {
    let cargo_exe = env::var("CARGO").unwrap();
    cmd!(cargo_exe, "new", "-q", "--vcs", "none", path).run()?;
    Ok(())
}

fn cargo_add_dep(cwd: &Path, package: &str, dep: &str) -> io::Result<()> {
    let cargo_exe = env::var("CARGO").unwrap();
    cmd!(cargo_exe, "add", "-p", package, dep).dir(cwd).run()?;
    Ok(())
}

fn assert_manifest(manifest_path: &Path, expected: &str) -> io::Result<()> {
    let modified = fs::read_to_string(manifest_path)?;
    assert_diff!(expected, &modified, "\n", 0);
    Ok(())
}

fn assert_stderr(stderr: &[u8], expected: &str) -> std::result::Result<(), Utf8Error> {
    assert_diff!(expected, str::from_utf8(stderr)?, "\n", 0);
    Ok(())
}

fn cargo_metadata(manifest_path: &Path, opts: &[&str]) -> cargo_metadata::Result<()> {
    let opts = opts
        .iter()
        .copied()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    MetadataCommand::new()
        .manifest_path(manifest_path)
        .other_options(opts.iter().map(ToOwned::to_owned).collect::<Vec<_>>())
        .exec()
        .map(drop)
}
