use std::process;

fn main() {
  let task = std::env::args().nth(1);
  match task.as_deref() {
    Some("check") => check_versions(),
    _ => {
      eprintln!("Usage: cargo xtask check");
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
  let cargo = read_file("Cargo.toml");
  let chart = read_file("helm/k8s-cloud-tagger/Chart.yaml");

  let cargo_version = extract_version(&cargo, "version =")
    .unwrap_or_else(|| { eprintln!("No version found in Cargo.toml"); process::exit(1) });
  let chart_version = extract_version(&chart, "version:")
    .unwrap_or_else(|| { eprintln!("No version found in Chart.yaml"); process::exit(1) });
  let app_version = extract_version(&chart, "appVersion:")
    .unwrap_or_else(|| { eprintln!("No appVersion found in Chart.yaml"); process::exit(1) });

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