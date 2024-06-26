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
fn cp() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-cp")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");

    let expected_stderr = EXPECTED_STDERR
        .replace("{{src}}", tempdir_path.join("ws").join("b").as_ref())
        .replace("{{dst}}", tempdir_path.join("b").as_ref());

    fs::create_dir(tempdir_path.join("ws"))?;
    fs::write(tempdir_path.join("ws").join("Cargo.toml"), MANIFEST)?;
    cargo_new(&tempdir_path.join("ws").join("a"))?;
    cargo_new(&tempdir_path.join("ws").join("b"))?;
    let metadata = cargo_metadata(&tempdir_path.join("ws").join("Cargo.toml"), &[])?;

    let mut stderr = vec![];

    cargo_member::Cp::from_metadata(&metadata, "b", &tempdir_path.join("b"))
        .dry_run(false)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    assert_manifest(&tempdir_path.join("ws").join("Cargo.toml"), MANIFEST)?;
    assert_stderr(&stderr, &expected_stderr)?;
    cargo_metadata(&tempdir_path.join("ws").join("Cargo.toml"), &["--locked"])?;
    cargo_metadata(&tempdir_path.join("b").join("Cargo.toml"), &["--locked"]).unwrap_err();
    cargo_metadata(&tempdir_path.join("b").join("Cargo.toml"), &[])?;
    return Ok(());

    static MANIFEST: &str = r#"[workspace]
members = ["a", "b"]
exclude = []
"#;

    static EXPECTED_STDERR: &str = r#"     Copying `{{src}}` to `{{dst}}`
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
