// This module checks GitHub for a newer Kingfisher release and (optionally)
// self‑updates.  Our release assets use short, user‑friendly names such as
// `kingfisher-linux-arm64.tgz`, `kingfisher-darwin-x64.tgz`, etc.  Those names
// do **not** match the full Rust target triple that the `self_update` crate
// expects (e.g. `aarch64-unknown-linux-musl`).  We therefore map the compile‑
// time target to the corresponding asset suffix via `builder.target()`.
//
// Version handling logic covers three scenarios:
//   1. Running version == latest release →                   "up to date".
//   2. Running version  > latest release → print a notice that the binary is **newer** than
//      anything on GitHub (e.g. a dev build).
//   3. Latest release  > running version → offer to self‑update.
//
// All informational messages are printed with the
// `style_finding_active_heading` style so that they stand out alongside normal
// scan output.

use std::{
    fs,
    io::{ErrorKind, IsTerminal},
    path::PathBuf,
};

use self_update::{backends::github::Update, cargo_crate_version, errors::Error as UpdError};
use semver::Version;
use tracing::{error, info, warn};

use crate::{cli::global::GlobalArgs, reporter::styles::Styles};

/// Return `true` when the canonical executable path lives inside a Homebrew Cellar.
/// Works for Intel macOS (/usr/local/Cellar), Apple‑Silicon macOS (/opt/homebrew/Cellar)
/// and Linuxbrew (~/.linuxbrew/Cellar).
fn installed_via_homebrew() -> bool {
    fn canonical_exe() -> Option<PathBuf> {
        std::env::current_exe().ok().and_then(|p| fs::canonicalize(p).ok())
    }

    canonical_exe().map(|p| p.components().any(|c| c.as_os_str() == "Cellar")).unwrap_or(false)
}

/// Check GitHub for a newer Kingfisher release and optionally self‑update.
///
/// * `base_url` lets tests point at a mock server.
/// * Self‑update is skipped when the user disabled it **or** the binary is a Homebrew install.
pub fn check_for_update(global_args: &GlobalArgs, base_url: Option<&str>) -> Option<String> {
    if global_args.no_update_check {
        return None;
    }

    // Decide once whether we want coloured output.
    let use_color = std::io::stderr().is_terminal() && !global_args.quiet;
    let styles = Styles::new(use_color);

    let is_brew = installed_via_homebrew();
    if is_brew {
        info!(
            "{}",
            styles.style_finding_active_heading.apply_to(
                "Homebrew install detected – will notify about updates but not self‑update"
            )
        );
    }

    info!("{}", "Checking for updates…");

    let mut builder = Update::configure();
    builder
        .repo_owner("mongodb")
        .repo_name("kingfisher")
        .bin_name("kingfisher")
        .show_download_progress(false)
        .no_confirm(true)  // Don't prompt for confirmation when self‑updating
        .current_version(cargo_crate_version!());

    // Allow tests to point at a mock HTTP server.
    if let Some(url) = base_url {
        builder.with_url(url);
    }

    // ──────────────────────────────────────────────────────
    // Map the current Rust target triple to our simplified asset names.
    // ──────────────────────────────────────────────────────
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    builder.target("linux-arm64");

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    builder.target("linux-x64");

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    builder.target("darwin-arm64");

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    builder.target("darwin-x64");

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    builder.target("windows-x64");

    // Build the updater.
    let Ok(updater) = builder.build() else {
        warn!("Failed to configure update checker");
        return None;
    };

    // Query GitHub.
    let Ok(release) = updater.get_latest_release() else {
        warn!("Failed to check for updates");
        return None;
    };

    let running_v = cargo_crate_version!();

    // ───────────── Case 1: running == latest ─────────────
    if release.version == running_v {
        let plain = format!("Kingfisher {running_v} is up to date");
        info!("{}", styles.style_finding_active_heading.apply_to(&plain));
        return Some(plain);
    }

    // Try semantic version comparison.  If parsing fails, fall back to the
    // self‑update code‑path (which will treat the strings lexicographically).
    if let (Ok(curr), Ok(latest)) = (Version::parse(running_v), Version::parse(&release.version)) {
        // ───────── Case 2: running > latest (dev build) ─────────
        if curr > latest {
            let plain =
                format!("Running Kingfisher {curr} which is newer than latest released {latest}");
            info!("{}", styles.style_finding_active_heading.apply_to(&plain));
            return Some(plain);
        }
        // else fall through to Case 3 (latest > running)
    }

    // ───────────── Case 3: latest > running ─────────────
    let plain = format!("New Kingfisher release {} available", release.version);
    info!("{}", styles.style_finding_active_heading.apply_to(&plain));

    // Attempt self‑update when allowed and feasible.
    if global_args.self_update && !is_brew {
        match updater.update() {
            Ok(status) => info!(
                "{}",
                styles
                    .style_finding_active_heading
                    .apply_to(&format!("Updated to version {}", status.version()))
            ),
            Err(e) => match e {
                UpdError::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied => {
                    warn!(
                        "{}",
                        styles.style_finding_active_heading.apply_to(
                            "Cannot replace the current binary – permission denied.\n\
                             If you installed via a package manager, run its upgrade command.\n\
                             Otherwise reinstall to a user‑writable directory or re‑run with sudo."
                        )
                    );
                }
                _ => error!("Failed to update: {e}"),
            },
        }
    } else if is_brew {
        info!(
            "{}",
            styles
                .style_finding_active_heading
                .apply_to("Run `brew upgrade kingfisher` to install the new version.")
        );
    }

    Some(plain)
}
