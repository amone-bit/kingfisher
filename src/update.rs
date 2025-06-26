use std::{
    fs,
    io::{ErrorKind, IsTerminal},
    path::PathBuf,
};

use self_update::{backends::github::Update, cargo_crate_version, errors::Error as UpdError};
use tracing::{error, info, warn};

use crate::cli::global::GlobalArgs;
use crate::reporter::styles::Styles;

/// Return `true` when the canonical executable path lives inside a Homebrew Cellar.
/// Works for Intel macOS (/usr/local/Cellar), Apple‑Silicon macOS (/opt/homebrew/Cellar)
/// and Linuxbrew (~/.linuxbrew/Cellar).
fn installed_via_homebrew() -> bool {
    fn canonical_exe() -> Option<PathBuf> {
        std::env::current_exe().ok().and_then(|p| fs::canonicalize(p).ok())
    }

    canonical_exe()
        .map(|p| p.components().any(|c| c.as_os_str() == "Cellar"))
        .unwrap_or(false)
}

/// Check GitHub for a newer Kingfisher release.
///
/// * `base_url` lets tests point at a mock server.
/// * Self‑update is performed unless the user disabled it **or** the binary is a Homebrew install.
pub fn check_for_update(global_args: &GlobalArgs, base_url: Option<&str>) -> Option<String> {
    if global_args.no_update_check {
        return None;
    }

    let is_brew = installed_via_homebrew();
    if is_brew {
        info!(
            "Homebrew install detected – will notify about updates but not self‑update"
        );
    }

    info!("Checking for updates…");

    // -------------------------------------------------------------
    // Prepare colour/style helper so every message looks consistent
    // -------------------------------------------------------------
    let use_color = std::io::stderr().is_terminal() && !global_args.quiet;
    let styles = Styles::new(use_color);

    let mut builder = Update::configure();
    builder
        .repo_owner("mongodb")
        .repo_name("kingfisher")
        .bin_name("kingfisher")
        .show_download_progress(false)
        .current_version(cargo_crate_version!());

    if let Some(url) = base_url {
        builder.with_url(url);
    }

    let Ok(updater) = builder.build() else {
        warn!("Failed to configure update checker");
        return None;
    };

    let Ok(release) = updater.get_latest_release() else {
        warn!("Failed to check for updates");
        return None;
    };

    // ----------------------------
    // Already on the latest version
    // ----------------------------
    if release.version == cargo_crate_version!() {
        let plain = format!("Kingfisher {} is up to date", release.version);
        let styled = styles.style_finding_active_heading.apply_to(&plain);
        info!("{}", styled);
        return Some(plain);
    }

    // ----------------------------
    // A newer version is available
    // ----------------------------
    let plain = format!("New Kingfisher release {} available", release.version);
    let styled = styles.style_finding_active_heading.apply_to(&plain);
    info!("{}", styled);

    // Decide whether to perform the update in place.
    if global_args.self_update && !is_brew {
        match updater.update() {
            Ok(status) => info!("Updated to version {}", status.version()),
            Err(e) => match e {
                UpdError::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied => {
                    warn!(
                        "Cannot replace the current binary – permission denied.\n\
                         If you installed via a package manager, run its upgrade command.\n\
                         Otherwise reinstall to a user‑writable directory or re‑run with sudo."
                    );
                }
                _ => error!("Failed to update: {e}"),
            },
        }
    } else if is_brew {
        info!("Run `brew upgrade kingfisher` to install the new version.");
    }

    Some(plain)
}
