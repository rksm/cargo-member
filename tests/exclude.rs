#![warn(rust_2018_idioms)]

use camino::Utf8Path as Path;
use cargo_metadata::{Metadata, MetadataCommand};
use duct::cmd;
use std::{env, fs, io, str};
use tempdir::TempDir;
use termcolor::NoColor;

#[test]
fn normal() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-exclude-normal")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");

    fs::write(
        tempdir_path.join("Cargo.toml"),
        r#"[workspace]
members = []
exclude = []
[workspace.dependencies]
bitflags = "2.6"
rand = "0.8"
"#,
    )?;
    cargo_new(&tempdir_path.join("a"))?;
    cargo_new(&tempdir_path.join("b"))?;
    cargo_new(&tempdir_path.join("c"))?;
    cargo_add_dep(tempdir_path, "b", "rand")?;
    cargo_add_dep(tempdir_path, "b", "bitflags@2.5")?;

    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("b").join("Cargo.toml")).unwrap());

    let metadata = cargo_metadata(&tempdir_path.join("Cargo.toml"), &[])?;

    let mut stderr = vec![];

    cargo_member::Exclude::from_metadata(&metadata, [tempdir_path.join("b")], ["c"])
        .dry_run(false)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("Cargo.toml")).unwrap());
    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("b").join("Cargo.toml")).unwrap());
    insta::assert_snapshot!(String::from_utf8_lossy(&stderr));
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &[])?;

    Ok(())
}

#[test]
fn dry_run() -> anyhow::Result<()> {
    let tempdir = TempDir::new("cargo-member-test-exclude-dry-run")?;
    let tempdir_path = Path::from_path(tempdir.path()).expect("invalid utf8 path");

    fs::write(tempdir_path.join("Cargo.toml"), MANIFEST)?;
    cargo_new(&tempdir_path.join("a"))?;
    cargo_new(&tempdir_path.join("b"))?;
    cargo_new(&tempdir_path.join("c"))?;
    cargo_add_dep(tempdir_path, "b", "rand")?;
    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("b").join("Cargo.toml")).unwrap());

    let metadata = cargo_metadata(&tempdir_path.join("Cargo.toml"), &[])?;

    let mut stderr = vec![];

    cargo_member::Exclude::from_metadata(&metadata, [tempdir_path.join("b")], ["c"])
        .dry_run(true)
        .stderr(NoColor::new(&mut stderr))
        .exec()?;

    insta::assert_snapshot!(fs::read_to_string(tempdir_path.join("Cargo.toml")).unwrap());
    insta::assert_snapshot!(String::from_utf8_lossy(&stderr));
    cargo_metadata(&tempdir_path.join("Cargo.toml"), &["--locked"])?;
    return Ok(());

    static MANIFEST: &str = r#"[workspace]
members = ["a", "b", "c"]
exclude = []
[workspace.dependencies]
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
