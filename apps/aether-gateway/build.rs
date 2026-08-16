use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AETHER_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=AETHER_BUILD_TYPE");
    println!("cargo:rerun-if-env-changed=AETHER_VERSION");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let package_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".to_string());
    let version = env::var("AETHER_BUILD_VERSION")
        .ok()
        .and_then(|value| normalize_gateway_version_source(&value))
        .or_else(|| {
            env::var("AETHER_VERSION")
                .ok()
                .and_then(|value| normalize_gateway_version_source(&value))
        })
        .or_else(|| {
            env::var("GITHUB_REF_NAME")
                .ok()
                .and_then(|value| normalize_gateway_version_source(&value))
        })
        .or_else(git_describe_version)
        .filter(|value| !value.is_empty())
        .unwrap_or(package_version);

    println!("cargo:rustc-env=AETHER_BUILD_VERSION={version}");

    let build_type = env::var("AETHER_BUILD_TYPE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "source".to_string());
    println!("cargo:rustc-env=AETHER_BUILD_TYPE={build_type}");
}

fn git_describe_version() -> Option<String> {
    let output = Command::new("git")
        .args([
            "describe", "--tags", "--match", "v[0-9]*", "--always", "--dirty",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    let version = version.trim();
    normalize_gateway_version_source(version)
}

fn normalize_gateway_version_source(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.strip_prefix('v').unwrap_or(trimmed).to_string())
}
