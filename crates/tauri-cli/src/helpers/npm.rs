// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use serde::Deserialize;

use crate::{
  error::{Context, Error},
  helpers::cross_command,
};
use std::{
  collections::{HashMap, HashSet},
  fmt::Display,
  path::{Path, PathBuf},
  process::Command,
};

pub fn manager_version(package_manager: &str) -> Option<String> {
  cross_command(package_manager)
    .arg("-v")
    .output()
    .map(|o| {
      if o.status.success() {
        let v = String::from_utf8_lossy(o.stdout.as_slice()).to_string();
        Some(v.split('\n').next().unwrap().to_string())
      } else {
        None
      }
    })
    .ok()
    .unwrap_or_default()
}

fn detect_yarn_or_berry() -> PackageManager {
  if manager_version("yarn")
    .map(|v| v.chars().next().map(|c| c > '1').unwrap_or_default())
    .unwrap_or(false)
  {
    PackageManager::YarnBerry
  } else {
    PackageManager::Yarn
  }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PackageManager {
  Npm,
  Pnpm,
  Yarn,
  YarnBerry,
  Bun,
  Deno,
}

impl Display for PackageManager {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "{}",
      match self {
        PackageManager::Npm => "npm",
        PackageManager::Pnpm => "pnpm",
        PackageManager::Yarn => "yarn",
        PackageManager::YarnBerry => "yarn berry",
        PackageManager::Bun => "bun",
        PackageManager::Deno => "deno",
      }
    )
  }
}

impl PackageManager {
  /// Detects package manager from the given directory.
  ///
  /// Falls back to [`PackageManager::Bun`] when it is installed, otherwise [`PackageManager::Npm`].
  pub fn from_project<P: AsRef<Path>>(path: P) -> Self {
    Self::all_from_project(path)
      .first()
      .copied()
      .unwrap_or_else(|| {
        if manager_version("bun").is_some() {
          Self::Bun
        } else {
          Self::Npm
        }
      })
  }

  /// Detection order when several lockfiles exist: bun first.
  fn priority(self) -> u8 {
    match self {
      Self::Bun => 0,
      Self::Deno => 1,
      Self::Pnpm => 2,
      Self::Yarn | Self::YarnBerry => 3,
      Self::Npm => 4,
    }
  }

  /// Detects package manager from the `npm_config_user_agent` environment variable
  fn from_environment_variable() -> Option<Self> {
    let npm_config_user_agent = std::env::var("npm_config_user_agent").ok()?;
    match npm_config_user_agent {
      user_agent if user_agent.starts_with("pnpm/") => Some(Self::Pnpm),
      user_agent if user_agent.starts_with("deno/") => Some(Self::Deno),
      user_agent if user_agent.starts_with("bun/") => Some(Self::Bun),
      user_agent if user_agent.starts_with("yarn/") => Some(detect_yarn_or_berry()),
      user_agent if user_agent.starts_with("npm/") => Some(Self::Npm),
      _ => None,
    }
  }

  /// Detects all possible package managers from the given directory.
  pub fn all_from_project<P: AsRef<Path>>(path: P) -> Vec<Self> {
    if let Some(from_env) = Self::from_environment_variable() {
      return vec![from_env];
    }

    let mut found = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
      for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        match name.as_ref() {
          "package-lock.json" => found.push(PackageManager::Npm),
          "pnpm-lock.yaml" => found.push(PackageManager::Pnpm),
          "yarn.lock" => found.push(detect_yarn_or_berry()),
          "bun.lock" | "bun.lockb" => found.push(PackageManager::Bun),
          "deno.lock" => found.push(PackageManager::Deno),
          _ => (),
        }
      }
    }

    // `read_dir` order is unspecified: sort so that bun wins over the other lockfiles
    found.sort_by_key(|pm| pm.priority());
    found.dedup();
    found
  }

  fn cross_command(&self) -> Command {
    match self {
      PackageManager::Yarn => cross_command("yarn"),
      PackageManager::YarnBerry => cross_command("yarn"),
      PackageManager::Npm => cross_command("npm"),
      PackageManager::Pnpm => cross_command("pnpm"),
      PackageManager::Bun => cross_command("bun"),
      PackageManager::Deno => cross_command("deno"),
    }
  }

  pub fn install<P: AsRef<Path>>(
    &self,
    dependencies: &[String],
    frontend_dir: P,
  ) -> crate::Result<()> {
    let dependencies_str = if dependencies.len() > 1 {
      "dependencies"
    } else {
      "dependency"
    };
    log::info!(
      "Installing NPM {dependencies_str} {}...",
      dependencies
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ")
    );

    let mut command = self.cross_command();
    command.arg("add");

    match self {
      PackageManager::Deno => command.args(dependencies.iter().map(|d| format!("npm:{d}"))),
      _ => command.args(dependencies),
    };

    let status = command
      .current_dir(frontend_dir)
      .status()
      .map_err(|error| Error::CommandFailed {
        command: format!("failed to run {self}"),
        error,
      })?;

    if !status.success() {
      crate::error::bail!("Failed to install NPM {dependencies_str}");
    }

    Ok(())
  }

  pub fn remove<P: AsRef<Path>>(
    &self,
    dependencies: &[String],
    frontend_dir: P,
  ) -> crate::Result<()> {
    let dependencies_str = if dependencies.len() > 1 {
      "dependencies"
    } else {
      "dependency"
    };
    log::info!(
      "Removing NPM {dependencies_str} {}...",
      dependencies
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ")
    );

    let status = self
      .cross_command()
      .arg(if *self == Self::Npm {
        "uninstall"
      } else {
        "remove"
      })
      .args(dependencies)
      .current_dir(frontend_dir)
      .status()
      .map_err(|error| Error::CommandFailed {
        command: format!("failed to run {self}"),
        error,
      })?;

    if !status.success() {
      crate::error::bail!("Failed to remove NPM {dependencies_str}");
    }

    Ok(())
  }

  // TODO: Use `current_package_versions` as much as possible for better speed
  pub fn current_package_version<P: AsRef<Path>>(
    &self,
    name: &str,
    frontend_dir: P,
  ) -> crate::Result<Option<String>> {
    let (output, regex) = match self {
      PackageManager::Yarn => (
        cross_command("yarn")
          .args(["list", "--pattern"])
          .arg(name)
          .args(["--depth", "0"])
          .current_dir(frontend_dir)
          .output()
          .map_err(|error| Error::CommandFailed {
            command: "yarn list --pattern".to_string(),
            error,
          })?,
        None,
      ),
      PackageManager::YarnBerry => (
        cross_command("yarn")
          .arg("info")
          .arg(name)
          .arg("--json")
          .current_dir(frontend_dir)
          .output()
          .map_err(|error| Error::CommandFailed {
            command: "yarn info --json".to_string(),
            error,
          })?,
        Some(regex::Regex::new("\"Version\":\"([\\da-zA-Z\\-\\.]+)\"").unwrap()),
      ),
      PackageManager::Pnpm => (
        cross_command("pnpm")
          .arg("list")
          .arg(name)
          .args(["--parseable", "--depth", "0"])
          .current_dir(frontend_dir)
          .output()
          .map_err(|error| Error::CommandFailed {
            command: "pnpm list --parseable --depth 0".to_string(),
            error,
          })?,
        None,
      ),
      PackageManager::Bun => {
        return Ok(
          bun_package_versions(&[name.to_owned()], frontend_dir.as_ref())?
            .remove(name)
            .map(|v| v.to_string()),
        );
      }
      // Deno doesn't support `list` command
      PackageManager::Npm | PackageManager::Deno => (
        cross_command("npm")
          .arg("list")
          .arg(name)
          .args(["version", "--depth", "0"])
          .current_dir(frontend_dir)
          .output()
          .map_err(|error| Error::CommandFailed {
            command: "npm list --version --depth 0".to_string(),
            error,
          })?,
        None,
      ),
    };
    if output.status.success() {
      let stdout = String::from_utf8_lossy(&output.stdout);
      let regex = regex.unwrap_or_else(|| regex::Regex::new("@(\\d[\\da-zA-Z\\-\\.]+)").unwrap());
      Ok(
        regex
          .captures_iter(&stdout)
          .last()
          .and_then(|cap| cap.get(1).map(|v| v.as_str().to_string())),
      )
    } else {
      Ok(None)
    }
  }

  pub fn current_package_versions(
    &self,
    packages: &[String],
    frontend_dir: &Path,
  ) -> crate::Result<HashMap<String, semver::Version>> {
    let output = match self {
      PackageManager::Yarn => return yarn_package_versions(packages, frontend_dir),
      PackageManager::YarnBerry => return yarn_berry_package_versions(packages, frontend_dir),
      PackageManager::Pnpm => cross_command("pnpm")
        .arg("list")
        .args(packages)
        .args(["--json", "--depth", "0"])
        .current_dir(frontend_dir)
        .output()
        .map_err(|error| Error::CommandFailed {
          command: "pnpm list --json --depth 0".to_string(),
          error,
        })?,
      PackageManager::Bun => return bun_package_versions(packages, frontend_dir),
      // Deno doesn't support `list` command
      PackageManager::Npm | PackageManager::Deno => cross_command("npm")
        .arg("list")
        .args(packages)
        .args(["--json", "--depth", "0"])
        .current_dir(frontend_dir)
        .output()
        .map_err(|error| Error::CommandFailed {
          command: "npm list --json --depth 0".to_string(),
          error,
        })?,
    };

    let mut versions = HashMap::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
      return Ok(versions);
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListOutput {
      #[serde(default)]
      dependencies: HashMap<String, ListDependency>,
      #[serde(default)]
      dev_dependencies: HashMap<String, ListDependency>,
    }

    #[derive(Deserialize)]
    struct ListDependency {
      version: String,
    }

    let json = if matches!(self, PackageManager::Pnpm) {
      serde_json::from_str::<Vec<ListOutput>>(&stdout)
        .ok()
        .and_then(|out| out.into_iter().next())
        .context("failed to parse pnpm list")?
    } else {
      serde_json::from_str::<ListOutput>(&stdout).context("failed to parse npm list")?
    };
    for (package, dependency) in json.dependencies.into_iter().chain(json.dev_dependencies) {
      let version = dependency.version;
      if let Ok(version) = semver::Version::parse(&version) {
        versions.insert(package, version);
      } else {
        log::debug!("Failed to parse version `{version}` for NPM package `{package}`");
      }
    }
    Ok(versions)
  }
}

fn yarn_package_versions(
  packages: &[String],
  frontend_dir: &Path,
) -> crate::Result<HashMap<String, semver::Version>> {
  let output = cross_command("yarn")
    .arg("list")
    .args(packages)
    .args(["--json", "--depth", "0"])
    .current_dir(frontend_dir)
    .output()
    .map_err(|error| Error::CommandFailed {
      command: "yarn list --json --depth 0".to_string(),
      error,
    })?;

  let mut versions = HashMap::new();
  let stdout = String::from_utf8_lossy(&output.stdout);
  if !output.status.success() {
    return Ok(versions);
  }

  #[derive(Deserialize)]
  struct YarnListOutput {
    data: YarnListOutputData,
  }

  #[derive(Deserialize)]
  struct YarnListOutputData {
    trees: Vec<YarnListOutputDataTree>,
  }

  #[derive(Deserialize)]
  struct YarnListOutputDataTree {
    name: String,
  }

  for line in stdout.lines() {
    if let Ok(tree) = serde_json::from_str::<YarnListOutput>(line) {
      for tree in tree.data.trees {
        let Some((name, version)) = tree.name.rsplit_once('@') else {
          continue;
        };
        if let Ok(version) = semver::Version::parse(version) {
          versions.insert(name.to_owned(), version);
        } else {
          log::debug!("Failed to parse version `{version}` for NPM package `{name}`");
        }
      }
      return Ok(versions);
    }
  }

  Ok(versions)
}

fn yarn_berry_package_versions(
  packages: &[String],
  frontend_dir: &Path,
) -> crate::Result<HashMap<String, semver::Version>> {
  let output = cross_command("yarn")
    .args(["info", "--json"])
    .current_dir(frontend_dir)
    .output()
    .map_err(|error| Error::CommandFailed {
      command: "yarn info --json".to_string(),
      error,
    })?;

  let mut versions = HashMap::new();
  let stdout = String::from_utf8_lossy(&output.stdout);
  if !output.status.success() {
    return Ok(versions);
  }

  #[derive(Deserialize)]
  struct YarnBerryInfoOutput {
    value: String,
    children: YarnBerryInfoOutputChildren,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "PascalCase")]
  struct YarnBerryInfoOutputChildren {
    version: String,
  }

  for line in stdout.lines() {
    if let Ok(info) = serde_json::from_str::<YarnBerryInfoOutput>(line) {
      let Some((name, _)) = info.value.rsplit_once('@') else {
        continue;
      };
      if !packages.iter().any(|package| package == name) {
        continue;
      }
      let version = info.children.version;
      if let Ok(version) = semver::Version::parse(&version) {
        versions.insert(name.to_owned(), version);
      } else {
        log::debug!("Failed to parse version `{version}` for NPM package `{name}`");
      }
    }
  }

  Ok(versions)
}

fn bun_package_versions(
  packages: &[String],
  frontend_dir: &Path,
) -> crate::Result<HashMap<String, semver::Version>> {
  let mut versions = HashMap::new();
  if packages.is_empty() {
    return Ok(versions);
  }

  let declared = read_bun_declared_deps(frontend_dir);
  let targets: Vec<&str> = packages
    .iter()
    .map(String::as_str)
    .filter(|pkg| declared.as_ref().is_none_or(|deps| deps.contains_key(*pkg)))
    .collect();
  if targets.is_empty() {
    return Ok(versions);
  }

  let target_set: HashSet<&str> = targets.iter().copied().collect();
  let workspace_dirs = bun_workspace_dirs(frontend_dir);

  // 1. Installed node_modules/<pkg>/package.json (walking workspace ancestors).
  for &pkg in &targets {
    if let Some(version) = read_node_modules_version(&workspace_dirs, pkg) {
      versions.insert(pkg.to_owned(), version);
    }
  }

  // 2. Resolved versions in bun.lock (JSONC text lockfile).
  if versions.len() < targets.len() {
    if let Some(lock_dir) = workspace_dirs
      .iter()
      .find(|dir| dir.join("bun.lock").is_file())
    {
      resolve_from_bun_lock(&lock_dir.join("bun.lock"), &target_set, &mut versions);
    }
  }

  // 3. Fallback to `bun pm ls` (handles binary bun.lockb or workspace hoisting) without invoking npm.
  if versions.len() < targets.len() {
    resolve_from_bun_pm_ls(frontend_dir, &target_set, &mut versions);
  }

  // 4. Final fallback: exact pinned semver in package.json (ignore ranges like `^2.0.0`).
  if let Some(declared_deps) = declared {
    for &pkg in &targets {
      if !versions.contains_key(pkg) {
        if let Some(spec) = declared_deps.get(pkg) {
          if let Ok(version) = semver::Version::parse(spec.trim()) {
            versions.insert(pkg.to_owned(), version);
          }
        }
      }
    }
  }

  Ok(versions)
}

fn bun_workspace_dirs(frontend_dir: &Path) -> Vec<PathBuf> {
  let mut dirs = Vec::new();
  for (depth, dir) in frontend_dir.ancestors().enumerate() {
    if depth > 8 {
      break;
    }
    let has_lock = dir.join("bun.lock").is_file() || dir.join("bun.lockb").is_file();
    let has_manifest = dir.join("package.json").is_file() || dir.join("node_modules").is_dir();
    if dirs.is_empty() || has_lock || has_manifest {
      dirs.push(dir.to_path_buf());
    }
    if has_lock || (depth > 0 && dir.join(".git").exists()) {
      break;
    }
  }
  dirs
}

fn read_bun_declared_deps(frontend_dir: &Path) -> Option<HashMap<String, String>> {
  let manifest_path = frontend_dir.join("package.json");
  if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
    if let Some(deps) = parse_package_json_deps(&raw) {
      return Some(deps);
    }
  }

  if manifest_path.is_file() {
    if let Ok(output) = cross_command("bun")
      .args([
        "pm",
        "pkg",
        "get",
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
      ])
      .current_dir(frontend_dir)
      .output()
    {
      if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(deps) = parse_package_json_deps(&stdout) {
          return Some(deps);
        }
      }
    }
  }

  None
}

fn parse_package_json_deps(raw: &str) -> Option<HashMap<String, String>> {
  #[derive(Deserialize, Default)]
  #[serde(rename_all = "camelCase")]
  struct PackageDeps {
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default)]
    dev_dependencies: HashMap<String, String>,
    #[serde(default)]
    optional_dependencies: HashMap<String, String>,
    #[serde(default)]
    peer_dependencies: HashMap<String, String>,
  }

  let parsed = serde_json::from_str::<PackageDeps>(raw)
    .or_else(|_| json5::from_str::<PackageDeps>(raw))
    .ok()?;
  let mut deps = HashMap::new();
  deps.extend(parsed.peer_dependencies);
  deps.extend(parsed.optional_dependencies);
  deps.extend(parsed.dev_dependencies);
  deps.extend(parsed.dependencies);
  Some(deps)
}

fn read_node_modules_version(workspace_dirs: &[PathBuf], pkg: &str) -> Option<semver::Version> {
  #[derive(Deserialize)]
  struct InstalledPackageJson {
    version: Option<String>,
  }

  for dir in workspace_dirs {
    let mut pkg_json_path = dir.join("node_modules");
    for segment in pkg.split('/') {
      pkg_json_path.push(segment);
    }
    pkg_json_path.push("package.json");
    if let Ok(raw) = std::fs::read_to_string(&pkg_json_path) {
      if let Ok(parsed) = serde_json::from_str::<InstalledPackageJson>(&raw) {
        if let Some(version) = parsed
          .version
          .and_then(|v| semver::Version::parse(v.trim()).ok())
        {
          return Some(version);
        }
      }
    }
  }
  None
}

fn resolve_from_bun_lock(
  lock_path: &Path,
  targets: &HashSet<&str>,
  versions: &mut HashMap<String, semver::Version>,
) {
  let Ok(raw) = std::fs::read_to_string(lock_path) else {
    return;
  };
  let Ok(lock) = json5::from_str::<serde_json::Value>(&raw) else {
    return;
  };
  let Some(packages_obj) = lock.get("packages").and_then(|v| v.as_object()) else {
    return;
  };

  fn parse_bun_lock_entry(val: &serde_json::Value) -> Option<(&str, semver::Version)> {
    let spec = val.as_array()?.first()?.as_str()?;
    let (name, ver) = spec.rsplit_once('@')?;
    let parsed = semver::Version::parse(ver).ok()?;
    Some((name, parsed))
  }

  for &pkg in targets {
    if versions.contains_key(pkg) {
      continue;
    }
    if let Some((name, ver)) = packages_obj.get(pkg).and_then(parse_bun_lock_entry) {
      if name == pkg {
        versions.insert(pkg.to_owned(), ver);
      }
    }
  }

  if versions.len() < targets.len() {
    for val in packages_obj.values() {
      if let Some((name, ver)) = parse_bun_lock_entry(val) {
        if targets.contains(name) && !versions.contains_key(name) {
          versions.insert(name.to_owned(), ver);
        }
      }
    }
  }
}

fn resolve_from_bun_pm_ls(
  frontend_dir: &Path,
  targets: &HashSet<&str>,
  versions: &mut HashMap<String, semver::Version>,
) {
  let Ok(output) = cross_command("bun")
    .args(["pm", "ls"])
    .current_dir(frontend_dir)
    .output()
  else {
    return;
  };
  if !output.status.success() {
    return;
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  for line in stdout.lines() {
    let trimmed = line.trim_start_matches(['├', '└', '│', '─', ' ']);
    let Some(token) = trimmed.split_whitespace().next() else {
      continue;
    };
    let Some((name, ver)) = token.rsplit_once('@') else {
      continue;
    };
    if targets.contains(name) && !versions.contains_key(name) {
      if let Ok(parsed) = semver::Version::parse(ver) {
        versions.insert(name.to_owned(), parsed);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::PackageManager;
  use std::fs;

  #[test]
  fn bun_reads_versions_from_package_json_and_bun_lock() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
      tmp.path().join("package.json"),
      r#"{
        "name": "app",
        "dependencies": { "@tauri-apps/api": "^2.0.0" },
        "devDependencies": { "@tauri-apps/cli": "^3.0.0-alpha.6" }
      }"#,
    )
    .unwrap();
    fs::write(
      tmp.path().join("bun.lock"),
      r#"{
        "lockfileVersion": 2,
        "packages": {
          "@tauri-apps/api": ["@tauri-apps/api@2.12.5", "", {}, "sha512-x"],
          "@tauri-apps/cli": ["@tauri-apps/cli@3.0.0-alpha.6", "", {}, "sha512-y"],
          "transitive-only": ["transitive-only@1.2.3", "", {}, "sha512-z"],
        },
      }"#,
    )
    .unwrap();

    assert_eq!(
      PackageManager::Bun
        .current_package_version("@tauri-apps/api", tmp.path())
        .unwrap(),
      Some("2.12.5".to_string())
    );
    assert_eq!(
      PackageManager::Bun
        .current_package_version("transitive-only", tmp.path())
        .unwrap(),
      None
    );

    let batch = PackageManager::Bun
      .current_package_versions(
        &[
          "@tauri-apps/api".to_string(),
          "@tauri-apps/cli".to_string(),
          "transitive-only".to_string(),
          "missing".to_string(),
        ],
        tmp.path(),
      )
      .unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(
      batch["@tauri-apps/api"],
      semver::Version::parse("2.12.5").unwrap()
    );
    assert_eq!(
      batch["@tauri-apps/cli"],
      semver::Version::parse("3.0.0-alpha.6").unwrap()
    );
  }

  #[test]
  fn bun_prefers_node_modules_and_falls_back_to_exact_package_json_version() {
    let root = tempfile::tempdir().unwrap();
    let app_dir = root.path().join("apps").join("desktop");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(
      root.path().join("package.json"),
      r#"{ "name": "mono", "private": true }"#,
    )
    .unwrap();
    fs::write(root.path().join("bun.lock"), r#"{ "packages": {} }"#).unwrap();
    fs::create_dir_all(root.path().join("node_modules/@tauri-apps/api")).unwrap();
    fs::write(
      root
        .path()
        .join("node_modules/@tauri-apps/api/package.json"),
      r#"{ "name": "@tauri-apps/api", "version": "2.11.0" }"#,
    )
    .unwrap();

    fs::write(
      app_dir.join("package.json"),
      r#"{
        "name": "desktop",
        "dependencies": {
          "@tauri-apps/api": "^2.0.0",
          "exact-pkg": "1.4.2",
          "range-only": "^1.0.0"
        }
      }"#,
    )
    .unwrap();

    let batch = PackageManager::Bun
      .current_package_versions(
        &[
          "@tauri-apps/api".to_string(),
          "exact-pkg".to_string(),
          "range-only".to_string(),
        ],
        &app_dir,
      )
      .unwrap();
    assert_eq!(
      batch.get("@tauri-apps/api"),
      Some(&semver::Version::parse("2.11.0").unwrap())
    );
    assert_eq!(
      batch.get("exact-pkg"),
      Some(&semver::Version::parse("1.4.2").unwrap())
    );
    assert!(!batch.contains_key("range-only"));
  }
}
