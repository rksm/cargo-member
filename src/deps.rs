use anyhow::{Context as _, Result};
use camino::Utf8Path as Path;
use toml_edit::Item;

fn unworkspaceify_dep(
    mut stderr: impl crate::WriteColorExt,
    name: &str,
    root_dir: impl AsRef<Path>,
    crate_dir: impl AsRef<Path>,
    ws_dep: &Item,
    crate_dep: &mut Item,
    changed: &mut bool,
) -> Result<()> {
    let root_dir = root_dir.as_ref();
    let crate_dir = crate_dir.as_ref();
    let is_optional = crate_dep
        .as_table_like()
        .and_then(|d| d.get("optional"))
        .and_then(|o| o.as_bool())
        .unwrap_or(false);

    *crate_dep = ws_dep.clone();

    if is_optional {
        crate_dep
            .as_table_like_mut()
            .unwrap()
            .insert("optional", toml_edit::value(true));
    }

    // If the workspace dependency has a `path` field, and it is relative, then
    // replace it with a relative path from the crate to the workspace root.
    if let Some(path) = ws_dep
        .as_table_like()
        .and_then(|d| d.get("path"))
        .and_then(|p| p.as_str())
    {
        let path = Path::new(path);
        if path.is_relative() {
            let path = root_dir.join(path);
            let new_dep_path = pathdiff::diff_utf8_paths(path, crate_dir);
            if let Some(new_dep_path) = new_dep_path {
                crate_dep
                    .as_table_like_mut()
                    .unwrap()
                    .insert("path", toml_edit::value(new_dep_path.to_string()));
            }
        }
    }

    let relative_path = crate_dir
        .strip_prefix(root_dir)
        .unwrap_or(crate_dir.as_ref());

    stderr.status("Updating", format!(r#""{name}" in {relative_path}"#))?;
    *changed = true;

    Ok(())
}

/// Updates the dependencies of the crate at `crate_dir` so that all workspace
/// dependencies are replaced with their versions from the workspace root. This
/// allows the crate to be built without the workspace.
pub(crate) fn unworkspaceify_deps(
    mut stderr: impl crate::WriteColorExt,
    root_dir: impl AsRef<Path>,
    crate_dir: impl AsRef<Path>,
    dry_run: bool,
) -> Result<()> {
    let crate_dir = crate_dir.as_ref();
    let root_dir = root_dir.as_ref();

    let root_cargo_toml = crate::fs::read_toml_edit(root_dir.join("Cargo.toml"))
        .with_context(|| format!("`{}` does not seem to be a package", root_dir))?;

    let Some(ws_deps) = root_cargo_toml
        .get("workspace")
        .and_then(|ws| ws.get("dependencies"))
    else {
        return Ok(());
    };

    let manifest_path = crate_dir.join("Cargo.toml");
    let mut cargo_toml = crate::fs::read_toml_edit(&manifest_path)
        .with_context(|| format!("`{}` does not seem to be a package", crate_dir))?;

    let mut changed = false;

    let mut modify_section = |section: &str| -> Result<()> {
        let deps = cargo_toml.get_mut(section).and_then(|d| d.as_table_mut());

        if let Some(deps) = deps {
            for (name, dep) in deps.iter_mut() {
                if !dep
                    .as_table_like()
                    .and_then(|dep| dep.get("workspace"))
                    .and_then(|ws| ws.as_bool())
                    .unwrap_or(false)
                {
                    // not a workspace dependency
                    continue;
                }
                if let Some(root_dep) = ws_deps.get(name.get()) {
                    unworkspaceify_dep(
                        &mut stderr,
                        name.get(),
                        root_dir,
                        crate_dir,
                        root_dep,
                        dep,
                        &mut changed,
                    )?;
                } else {
                    stderr.warn(format!("No `{name}` in workspace"))?;
                }
            }
        }

        Ok(())
    };

    modify_section("dependencies")?;
    modify_section("dev-dependencies")?;
    modify_section("build-dependencies")?;

    if changed {
        crate::fs::write(manifest_path, cargo_toml.to_string(), dry_run)?;
    }

    Ok(())
}

struct LockFileVersions {
    versions_by_package_name: std::collections::HashMap<String, String>,
}

impl LockFileVersions {
    // pub fn new(crate_dir: impl AsRef<Path>) -> Self {
    //     Self {
    //         versions_by_package_name: Default::default(),
    //     }
    // }

    pub fn new(crate_dir: impl AsRef<Path>) -> Self {
        #[derive(serde::Deserialize)]
        struct LockFile {
            package: Vec<Package>,
        }

        #[derive(serde::Deserialize)]
        struct Package {
            name: String,
            version: String,
        }

        let lock_file_path = crate_dir.as_ref().join("Cargo.lock");
        let Some(lock_file) = crate::fs::read_toml::<LockFile, _>(&lock_file_path).ok() else {
            return Self {
                versions_by_package_name: std::collections::HashMap::new(),
            };
        };

        let versions_by_name = lock_file
            .package
            .into_iter()
            .map(|p| (p.name, p.version))
            .collect::<std::collections::HashMap<_, _>>();

        Self {
            versions_by_package_name: versions_by_name,
        }
    }

    fn lookup(&self, package_name: &str) -> Option<&str> {
        self.versions_by_package_name
            .get(package_name)
            .map(String::as_str)
    }
}

#[allow(clippy::too_many_arguments)]
fn workspaceify_dep(
    mut stderr: impl crate::WriteColorExt,
    name: &str,
    root_dir: impl AsRef<Path>,
    crate_dir: impl AsRef<Path>,
    ws_dep: &Item,
    crate_dep: &mut Item,
    changed: &mut bool,
    lock_file: &LockFileVersions,
) -> Result<()> {
    let root_dir = root_dir.as_ref();
    let crate_dir = crate_dir.as_ref();

    // check if version matches accrording to semver
    let root_version_req = ws_dep
        .get("version")
        .and_then(|v| v.as_str())
        .or_else(|| ws_dep.as_str())
        .and_then(|v| semver::VersionReq::parse(v).ok());

    // Stuff is complicated... in order to test if the VersionReq of the
    // workspace matches, we need a proper semver version, not just a VersionReq
    // (which is what gets specified in the Cargo.toml file). So we will first
    // try to lookup the dependency in the Cargo.lock file if we have one. If
    // not, we will try to parse the version. This works for simple versions
    // such as "1.2.3" or "1.2" or "1", but not for more complex version
    // requirements. It would be nice if VersionReq would allow us to "sample"
    // versions but it does not right now.
    let crate_dep_version = lock_file
        .lookup(name)
        .and_then(|v| semver::Version::parse(v).ok())
        .or_else(|| {
            crate_dep
                .get("version")
                .and_then(|v| v.as_str())
                .or_else(|| ws_dep.as_str())
                .and_then(|v| {
                    semver::Version::parse(v).ok().or_else(|| {
                        let re = regex::Regex::new(
                            r#"^(?P<major>\d+)(?:\.(?P<minor>\d+))?(\.(?P<patch>\d+))?$"#,
                        )
                        .unwrap();
                        let captures = re.captures(v)?;
                        let major = captures.name("major")?.as_str().parse().ok()?;
                        let minor = captures
                            .name("minor")
                            .and_then(|m| m.as_str().parse().ok())
                            .unwrap_or(0);
                        let patch = captures
                            .name("patch")
                            .and_then(|m| m.as_str().parse().ok())
                            .unwrap_or(0);
                        Some(semver::Version::new(major, minor, patch))
                    })
                })
        });

    match (root_version_req, crate_dep_version) {
        (Some(root_version_req), Some(dep_version)) => {
            if !root_version_req.matches(&dep_version) {
                stderr.warn(format!(r#"Skipping dependency "{name}": version mismatch"#))?;
                return Ok(());
            }
        }
        (Some(_), _) | (_, Some(_)) => {
            stderr.warn(format!(
                r#"Skipping dependency "{name}": no version in workspace/crate"#
            ))?;
            return Ok(());
        }
        _ => {}
    }

    // If we have a path we need to check if the path between the workspace and
    // and root toml point to the same location
    let root_dep_path = ws_dep.get("path").and_then(|v| v.as_str()).map(Path::new);

    let crate_dep_path = crate_dep
        .get("path")
        .and_then(|v| v.as_str())
        .map(Path::new);

    if let (Some(root_dep_path), Some(crate_dep_path)) = (root_dep_path, crate_dep_path) {
        let root_dep_path = if root_dep_path.is_relative() {
            root_dir.join(root_dep_path)
        } else {
            root_dep_path.to_path_buf()
        };

        let crate_dep_path = if crate_dep_path.is_relative() {
            crate_dir.join(crate_dep_path)
        } else {
            crate_dep_path.to_path_buf()
        };

        if root_dep_path.canonicalize()? != crate_dep_path.canonicalize()? {
            dbg!((root_dep_path, crate_dep_path));
            stderr.warn(format!(r#"Skipping dependency "{name}": path mismatch"#))?;
            return Ok(());
        }
    }

    // let mut new_item = toml_edit::table();
    // let table = new_item.as_table_mut().unwrap();
    // table.insert("workspace", toml_edit::value(true));
    // table.set_dotted(true);
    // *crate_dep = new_item;

    if crate_dep.is_str() {
        *crate_dep = toml_edit::table();
    }

    if let Some(table) = crate_dep.as_table_like_mut() {
        table.insert("workspace", toml_edit::value(true));
        table.remove("version");
        table.remove("path");
        // Remove properties that are the same as the workspace dependency
        let keys = table.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
        for key in keys {
            if key == "optional" {
                continue;
            }
            match (table.get(&key), ws_dep.get(&key)) {
                (Some(a), Some(b)) if a.to_string() == b.to_string() => {
                    table.remove(&key);
                }
                _ => {}
            }
        }
        if table.len() == 1 {
            table.set_dotted(true);
        }
    }

    // stderr.status(
    //     "Updating",
    //     format!(r#""{name}" in {relative_manifest_path}"#),
    // )?;
    *changed = true;

    Ok(())
}

pub(crate) fn workspaceify_deps(
    mut stderr: impl crate::WriteColorExt,
    root_dir: impl AsRef<Path>,
    crate_dir: impl AsRef<Path>,
    dry_run: bool,
) -> Result<()> {
    let crate_dir = crate_dir.as_ref();
    let root_dir = root_dir.as_ref();

    let root_cargo_toml = crate::fs::read_toml_edit(root_dir.join("Cargo.toml"))
        .with_context(|| format!("`{}` does not seem to be a package", root_dir))?;

    let Some(ws_deps) = root_cargo_toml
        .get("workspace")
        .and_then(|ws| ws.get("dependencies"))
    else {
        return Ok(());
    };

    let manifest_path = crate_dir.join("Cargo.toml");
    let mut cargo_toml = crate::fs::read_toml_edit(&manifest_path)
        .with_context(|| format!("`{}` does not seem to be a package", crate_dir))?;

    let lock_file = LockFileVersions::new(crate_dir);

    let mut changed = false;

    let mut modify_section = |section| -> Result<()> {
        let deps = cargo_toml.get_mut(section).and_then(|d| d.as_table_mut());

        if let Some(deps) = deps {
            for (name, dep) in deps.iter_mut() {
                let Some(root_dep) = ws_deps.get(name.get()) else {
                    // dependency not in workspace
                    continue;
                };

                workspaceify_dep(
                    &mut stderr,
                    name.get(),
                    root_dir,
                    crate_dir,
                    root_dep,
                    dep,
                    &mut changed,
                    &lock_file,
                )?;
            }
        }

        Ok(())
    };

    modify_section("dependencies")?;
    modify_section("dev-dependencies")?;
    modify_section("build-dependencies")?;

    if changed {
        crate::fs::write(manifest_path, cargo_toml.to_string(), dry_run)?;
    }

    Ok(())
}
