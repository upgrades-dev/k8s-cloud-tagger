use std::process;

const CARGO_FILE_PATH: &str = "Cargo.toml";
const CARGO_LOCK_FILE_PATH: &str = "Cargo.lock";
const HELM_CHART_FILE_PATH: &str = "helm/k8s-cloud-tagger/Chart.yaml";

fn main() {
  let task = std::env::args().nth(1);
  match task.as_deref() {
    Some("check") => check_versions(),
    Some("release") => {
      let version = std::env::args().nth(2).unwrap_or_else(|| {
        eprintln!("Usage: cargo xtask release <x.y.z>");
        process::exit(1);
      });
      release(&version);
    }
    _ => {
      eprintln!("Usage: cargo xtask <check|release>");
      process::exit(1);
    }
  }
}

fn read_file(path: &str) -> String {
  std::fs::read_to_string(path).unwrap_or_else(|e| {
    eprintln!("Failed to read {path}: {e}");
    process::exit(1);
  })
}

fn extract_version<'a>(content: &'a str, prefix: &str) -> Option<&'a str> {
  content.lines().find_map(|line| {
    let line = line.trim();
    let rest = line.strip_prefix(prefix)?;
    Some(rest.trim().trim_matches('"'))
  })
}

fn check_versions() {
  let cargo = read_file(CARGO_FILE_PATH);
  let chart = read_file(HELM_CHART_FILE_PATH);

  let cargo_version = extract_version(&cargo, "version =")
    .unwrap_or_else(|| { eprintln!("No version found in {CARGO_FILE_PATH}"); process::exit(1) });
  let chart_version = extract_version(&chart, "version:")
    .unwrap_or_else(|| { eprintln!("No version found in {HELM_CHART_FILE_PATH}"); process::exit(1) });
  let app_version = extract_version(&chart, "appVersion:")
    .unwrap_or_else(|| { eprintln!("No appVersion found in {HELM_CHART_FILE_PATH}"); process::exit(1) });

  let ok = cargo_version == chart_version && cargo_version == app_version;

  println!("Cargo.toml:  {cargo_version}");
  println!("Chart version:     {chart_version}");
  println!("Chart appVersion:  {app_version}");

  if ok {
    println!("OK: all versions match");
  } else {
    eprintln!("ERROR: versions do not match");
    process::exit(1);
  }
}

fn release(version: &str) {
  let new = parse_semver(version).unwrap_or_else(|| {
    eprintln!("Invalid version: {version}. Expected format: x.y.z");
    process::exit(1);
  });

  // Check that the new version is an upgrade (downgrade not allowed)
  let current_content = read_file(CARGO_FILE_PATH);
  let current_str = extract_version(&current_content, "version = ").unwrap_or_else(|| {
    eprintln!("Could not read current version from {CARGO_FILE_PATH}");
    process::exit(1);
  });
  let current = parse_semver(current_str).unwrap_or_else(|| {
    eprintln!("Could not parse current version: {current_str}");
    process::exit(1);
  });

  // Fine for as long as we stick to x.y.z
  // If we start doing 1.0.0-alpha releases we should us a parsing lib
  if current >= new {
    eprintln!("New version {version} must be greater than current {current_str}");
    process::exit(1);
  }

  bump_file(CARGO_FILE_PATH, "version", version);
  bump_file(HELM_CHART_FILE_PATH, "version", version);
  bump_file(HELM_CHART_FILE_PATH, "appVersion", version);

  run("cargo", &["generate-lockfile"]);
  run("git", &["add", CARGO_FILE_PATH, CARGO_LOCK_FILE_PATH, HELM_CHART_FILE_PATH]);
  run("git", &["commit", "--message", &format!("release v{version}")]);
  run("git", &["tag", &format!("v{version}")]);

  println!("Done. Run: git push && git push --tags");
}

fn parse_semver(v: &str) -> Option<(u32, u32, u32)> {
  let parts: Vec<&str> = v.split('.').collect();
  if parts.len() != 3 {
    return None;
  }
  Some((
    parts[0].parse().ok()?,
    parts[1].parse().ok()?,
    parts[2].parse().ok()?,
  ))
}

fn bump_file(path: &str, key: &str, version: &str) {
  let content = read_file(path);
  let prefix = if path.ends_with(".toml") {
    format!("{key} = ")
  } else {
    format!("{key}: ")
  };
  let new_content = content
    .lines()
    .map(|line| {
      if line.trim().starts_with(&prefix) {
        let indent = &line[..line.len() - line.trim_start().len()];
        if path.ends_with(".toml") {
          format!("{indent}{key} = \"{version}\"")
        } else {
          format!("{indent}{key}: {version}")
        }
      } else {
        line.to_string()
      }
    })
    .collect::<Vec<_>>()
    .join("\n") + "\n";

  std::fs::write(path, new_content).unwrap_or_else(|e| {
    eprintln!("Failed to write {path}: {e}");
    process::exit(1);
  });
  println!("Updated {path}");
}

fn run(cmd: &str, args: &[&str]) {
  let status = process::Command::new(cmd)
    .args(args)
    .status()
    .unwrap_or_else(|e| {
      eprintln!("Failed to run {cmd}: {e}");
      process::exit(1);
    });
  if !status.success() {
    eprintln!("Command failed: {cmd} {}", args.join(" "));
    process::exit(1);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_semver() {
    assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
    assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
    assert_eq!(parse_semver("1.2"), None);
    assert_eq!(parse_semver("1.2.x"), None);
    assert_eq!(parse_semver(""), None);
  }
}
