use colored::Colorize;
use reqwest::blocking::Client;
use reqwest::{Error as ReqwestError, StatusCode};
use semver::Version;
use std::error::Error;

/// URL to the remote Cargo.toml file to check for the latest version
const REMOTE_CARGO_TOML_URL: &str =
    "https://ghproxy.cc/https://raw.githubusercontent.com/louis-e/arnis/main/Cargo.toml";

/// Fetches the latest version from the remote Cargo.toml file and compares it with the local version.
/// Returns `true` if a newer version is available, `false` otherwise.
pub fn check_for_updates() -> Result<bool, Box<dyn Error>> {
    let client: Client = Client::new();

    // Fetch the remote Cargo.toml file with a User-Agent header
    let response: Result<reqwest::blocking::Response, ReqwestError> = client
        .get(REMOTE_CARGO_TOML_URL)
        .header("User-Agent", "arnis-client")
        .send();

    match response {
        Ok(res) => {
            // If the response status is not 200 OK, handle it as an HTTP error
            if !res.status().is_success() {
                handle_http_error(res.status());
                return Ok(false);
            }

            let response_text: String = res.text()?;
            // Extract the version from the remote Cargo.toml
            let remote_version: Version = extract_version_from_cargo_toml(&response_text)?;
            let local_version: Version = Version::parse(env!("CARGO_PKG_VERSION"))?;

            // Compare versions
            if remote_version > local_version {
                println!(
                    "{} {} -> {}",
                    "有新版本可用：".yellow().bold(),
                    local_version,
                    remote_version
                );
                return Ok(true); // Newer version is available
            }

            Ok(false) // Local version is up-to-date
        }
        Err(err) => {
            handle_request_error(err);
            Ok(false) // Treat request failures as no new version available
        }
    }
}

/// Extracts the version from the contents of a Cargo.toml file.
fn extract_version_from_cargo_toml(cargo_toml_contents: &str) -> Result<Version, Box<dyn Error>> {
    for line in cargo_toml_contents.lines() {
        if line.starts_with("version") {
            let version_str: &str = line.split('=').nth(1).unwrap().trim().trim_matches('"');
            return Ok(Version::parse(version_str)?);
        }
    }
    Err("在 Cargo.toml 中找不到版本".into())
}

/// Handles HTTP errors by printing the status code and a user-friendly message.
fn handle_http_error(status: StatusCode) {
    eprintln!(
        "无法获取远程 Cargo.toml：HTTP 错误 {}：{}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("未知错误")
    );
}

/// Handles the error for HTTP requests more gracefully, including printing HTTP status codes when applicable.
fn handle_request_error(err: ReqwestError) {
    if err.is_timeout() {
        eprintln!("请求超时。请检查您的网络连接。");
    } else if let Some(status) = err.status() {
        handle_http_error(status);
    }
}
