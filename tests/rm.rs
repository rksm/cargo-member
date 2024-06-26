#![warn(rust_2018_idioms)]

use camino::Utf8Path as Path;
use cargo_metadata::{Metadata, MetadataCommand};
use difference::assert_diff;
use duct::cmd;
use std::{
    env, fs, io,
    str::{self, Utf8Error},
};
use tempdir::TempDir;
use termcolor::NoColor;

#[test]
fn rm() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-rm")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");

    let expected_stderr = EXPECTED_STDERR
        .replace("{{b}}", tempdir_path.join("b").as_ref())
        .replace("{{c}}", tempdir_path.join("c").as_ref());

    fs::write(tempdir_path.join("Cargo.toml"), ORIGINAL)?;
    cargo_new(&tempdir_path.join("a"))?;
    cargo_new(&tempdir_path.join("b"))?;
    cargo_new(&tempdir_path.join("c"))?;
    let metadata = cargo_metadata(&tempdir_path.join("Cargo.toml"), &[])?;

    let mut stderr = vec![];

    cargo_member::Rm::from_metadata(&metadata, [tempdir_path.join("b")], ["c"])
        .force(false)
        .dry_run(false)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    assert_manifest(&tempdir_path.join("Cargo.toml"), EXPECTED_MANIFEST)?;
    assert_stderr(&stderr, &expected_stderr)?;
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &["--locked"])?;
    return Ok(());

    static ORIGINAL: &str = r#"[workspace]
members = ["a", "b", "c"]
exclude = []
"#;

    static EXPECTED_MANIFEST: &str = r#"[workspace]
members = ["a"]
exclude = []
"#;

    static EXPECTED_STDERR: &str = r#"    Removing directory `{{b}}`
    Removing "b" from `workspace.members`
    Removing directory `{{c}}`
    Removing "c" from `workspace.members`
"#;
}

fn cargo_new(path: &Path) -> io::Result<()> {
    let cargo_exe = env::var("CARGO").unwrap();
    cmd!(cargo_exe, "new", "-q", "--vcs", "none", path).run()?;
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

fn cargo_metadata(manifest_path: &Path, opts: &[&str]) -> cargo_metadata::Result<Metadata> {
    let opts = opts
        .iter()
        .copied()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    MetadataCommand::new()
        .manifest_path(manifest_path)
        .other_options(opts.iter().map(ToOwned::to_owned).collect::<Vec<_>>())
        .exec()
}
