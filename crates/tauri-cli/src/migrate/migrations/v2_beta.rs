// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::{
  Result,
  error::{Context, ErrorExt},
  helpers::{app_paths::Dirs, npm::PackageManager},
  interface::rust::manifest::{read_manifest, serialize_manifest},
};

use std::{fs::read_to_string, path::Path};

use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

pub fn run(dirs: &Dirs) -> Result<()> {
  let manifest_path = dirs.tauri.join("Cargo.toml");
  let (mut manifest, _) = read_manifest(&manifest_path)?;
  migrate_manifest(&mut manifest)?;

  migrate_permissions(dirs.tauri)?;

  migrate_npm_dependencies(dirs.frontend)?;

  std::fs::write(&manifest_path, serialize_manifest(&manifest))
    .fs_context("failed to rewrite Cargo manifest", &manifest_path)?;

  Ok(())
}

fn migrate_npm_dependencies(frontend_dir: &Path) -> Result<()> {
  let pm = PackageManager::from_project(frontend_dir);

  let mut install_deps = Vec::new();
  for pkg in [
    "@aphrody/cli",
    "@aphrody/api",
    "@aphrody/plugin-authenticator",
    "@aphrody/plugin-autostart",
    "@aphrody/plugin-barcode-scanner",
    "@aphrody/plugin-biometric",
    "@aphrody/plugin-cli",
    "@aphrody/plugin-clipboard-manager",
    "@aphrody/plugin-deep-link",
    "@aphrody/plugin-dialog",
    "@aphrody/plugin-fs",
    "@aphrody/plugin-global-shortcut",
    "@aphrody/plugin-http",
    "@aphrody/plugin-log",
    "@aphrody/plugin-nfc",
    "@aphrody/plugin-notification",
    "@aphrody/plugin-os",
    "@aphrody/plugin-positioner",
    "@aphrody/plugin-process",
    "@aphrody/plugin-shell",
    "@aphrody/plugin-sql",
    "@aphrody/plugin-store",
    "@aphrody/plugin-stronghold",
    "@aphrody/plugin-updater",
    "@aphrody/plugin-upload",
    "@aphrody/plugin-websocket",
    "@aphrody/plugin-window-state",
  ] {
    let version = pm
      .current_package_version(pkg, frontend_dir)
      .unwrap_or_default()
      .unwrap_or_default();
    if version.starts_with('1') {
      install_deps.push(format!("{pkg}@^2.0.0"));
    }
  }

  if !install_deps.is_empty() {
    pm.install(&install_deps, frontend_dir)?;
  }

  Ok(())
}

fn migrate_permissions(tauri_dir: &Path) -> Result<()> {
  let core_plugins = [
    "app",
    "event",
    "image",
    "menu",
    "path",
    "resources",
    "tray",
    "webview",
    "window",
  ];

  for entry in walkdir::WalkDir::new(tauri_dir.join("capabilities")) {
    let entry = entry.map_err(std::io::Error::other).fs_context(
      "failed to walk capabilities directory",
      tauri_dir.join("capabilities"),
    )?;
    let path = entry.path();
    if path.extension().is_some_and(|ext| ext == "json") {
      let mut capability =
        read_to_string(path).fs_context("failed to read capability", path.to_path_buf())?;
      for plugin in core_plugins {
        capability = capability.replace(&format!("\"{plugin}:"), &format!("\"core:{plugin}:"));
      }
      std::fs::write(path, capability)
        .fs_context("failed to rewrite capability", path.to_path_buf())?;
    }
  }
  Ok(())
}

fn migrate_manifest(manifest: &mut DocumentMut) -> Result<()> {
  let version = "2.0.0";

  let dependencies = manifest
    .as_table_mut()
    .entry("dependencies")
    .or_insert(Item::Table(Table::new()))
    .as_table_mut()
    .context("manifest dependencies isn't a table")?;

  for dep in [
    "tauri",
    "tauri-plugin-authenticator",
    "tauri-plugin-autostart",
    "tauri-plugin-barcode-scanner",
    "tauri-plugin-biometric",
    "tauri-plugin-cli",
    "tauri-plugin-clipboard-manager",
    "tauri-plugin-deep-link",
    "tauri-plugin-dialog",
    "tauri-plugin-fs",
    "tauri-plugin-global-shortcut",
    "tauri-plugin-http",
    "tauri-plugin-localhost",
    "tauri-plugin-log",
    "tauri-plugin-nfc",
    "tauri-plugin-notification",
    "tauri-plugin-os",
    "tauri-plugin-persisted-scope",
    "tauri-plugin-positioner",
    "tauri-plugin-process",
    "tauri-plugin-shell",
    "tauri-plugin-single-instance",
    "tauri-plugin-sql",
    "tauri-plugin-store",
    "tauri-plugin-stronghold",
    "tauri-plugin-updater",
    "tauri-plugin-upload",
    "tauri-plugin-websocket",
    "tauri-plugin-window-state",
  ] {
    migrate_dependency(dependencies, dep, version);
  }

  let build_dependencies = manifest
    .as_table_mut()
    .entry("build-dependencies")
    .or_insert(Item::Table(Table::new()))
    .as_table_mut()
    .context("manifest build-dependencies isn't a table")?;

  migrate_dependency(build_dependencies, "tauri-build", version);

  Ok(())
}

fn migrate_dependency(dependencies: &mut Table, name: &str, version: &str) {
  let item = dependencies.entry(name).or_insert(Item::None);

  // do not rewrite if dependency uses workspace inheritance
  if item
    .get("workspace")
    .and_then(|v| v.as_bool())
    .unwrap_or_default()
  {
    log::info!(
      "`{name}` dependency has workspace inheritance enabled. The features array won't be automatically rewritten."
    );
    return;
  }

  if let Some(dep) = item.as_table_mut() {
    migrate_dependency_table(dep, version);
  } else if let Some(Value::InlineTable(table)) = item.as_value_mut() {
    migrate_dependency_table(table, version);
  } else if item.as_str().is_some() {
    *item = Item::Value(version.into());
  }
}

fn migrate_dependency_table<D: TableLike>(dep: &mut D, version: &str) {
  *dep.entry("version").or_insert(Item::None) = Item::Value(version.into());
}
