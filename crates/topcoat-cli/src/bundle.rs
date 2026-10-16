use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Args;
use tokio::process::Command;

use crate::{
    asset::run_bundle,
    common::cargo::{BuildError, BuildFlags, BuildOpts, Metadata},
};

mod identity;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/iroh_bundle.rs"));
}

type Error = Box<dyn std::error::Error + Send + Sync>;

const MARKER: &str = ".topcoat-deploy-bundle";
// Match the marker retained by IrohService::new in optimized builds.
const IROH_MARKER: &[u8] = b"\0TOPCOAT_IROH_HTTP_V1\0";
const START_MARKER: &[u8] = b"\0TOPCOAT_IROH_START_V1\0";
// Match the registration emitted by #[launcher], including optimized builds.
const LAUNCHER_MARKER: &[u8] = b"\0TOPCOAT_LAUNCHER_V1\0";

#[derive(Args)]
pub struct BundleCommand {
    #[command(flatten)]
    build: BuildFlags,
    /// Output directory (defaults to the built executable's directory)
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Server identity key (otherwise reuse the saved key or generate one)
    #[arg(long)]
    iroh_secret: Option<String>,
    /// Public server endpoint (without --iroh-secret, require `TOPCOAT_IROH_SECRET` at startup)
    #[arg(long, conflicts_with = "iroh_ticket")]
    iroh_endpoint: Option<String>,
    /// Public server ticket, instead of --iroh-endpoint
    #[arg(long)]
    iroh_ticket: Option<String>,
    /// Allow this client endpoint ID (repeat to allow multiple peers)
    #[arg(long = "iroh-peer", value_name = "ID")]
    iroh_peers: Vec<String>,
}

impl BundleCommand {
    pub async fn run(self) {
        if let Err(error) = run(self).await {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

async fn run(args: BundleCommand) -> Result<(), Error> {
    let metadata = Metadata::workspace()
        .await
        .ok_or("could not read Cargo metadata")?;
    let name = metadata
        .build_package(args.build.package.as_deref())
        .ok_or("select an application package with --package")?;
    let target = metadata
        .target_dir()
        .ok_or("could not determine Cargo target directory")?;
    let mut build = BuildOpts::from(args.build);
    build.package = Some(name.clone());
    build.profile.get_or_insert_with(|| "release".to_owned());
    build.bin = build
        .bin
        .or_else(|| metadata.default_run_for_build(Some(&name)));
    let identity_path = target
        .join("topcoat/identities")
        .join(&name)
        .join(format!("{}.json", build.bin.as_deref().unwrap_or(&name)));
    let identity = identity::ServerIdentity::load(
        &identity_path,
        args.iroh_secret.as_deref(),
        args.iroh_endpoint.as_deref(),
        args.iroh_ticket.as_deref(),
        &args.iroh_peers,
    )?;
    build.iroh = Some(serde_json::to_string(&identity)?);
    println!("building server...");
    let (exe, bytes) = build.build_and_read(|_, _| {}).await.map_err(build_error)?;
    let profile_dir = exe.parent().ok_or("executable has no parent directory")?;
    let out = std::path::absolute(args.out.as_deref().unwrap_or(profile_dir))?;
    let beside_executable = out == profile_dir;
    if !beside_executable && out.exists() && (!out.join(MARKER).is_file() || out.is_symlink()) {
        return Err(format!(
            "{} is not a generated deployment bundle; choose an empty output path",
            out.display()
        )
        .into());
    }
    fs::create_dir_all(out.parent().ok_or("output has no parent")?)?;
    let staging = tempfile::Builder::new()
        .prefix(".topcoat-bundle-")
        .tempdir_in(out.parent().unwrap())?;
    let bundle = staging.path().join("bundle");
    let launcher = bundle.join("launcher");
    fs::create_dir_all(&bundle)?;
    run_bundle(&exe, &bytes, Some(bundle.join("assets"))).await?;
    let executable_name = exe.file_name().ok_or("executable has no name")?;
    if !beside_executable {
        fs::copy(&exe, bundle.join(executable_name))?;
    }

    let custom_launcher = contains_marker(&bytes, LAUNCHER_MARKER);
    let automatic_start = contains_marker(&bytes, START_MARKER);
    let iroh = custom_launcher || contains_marker(&bytes, IROH_MARKER);
    if !automatic_start && (args.iroh_secret.is_some() || !args.iroh_peers.is_empty()) {
        return Err("server identity and peer options require this fork's topcoat::start".into());
    }
    if automatic_start {
        if args.iroh_secret.is_none() && args.iroh_endpoint.is_none() && args.iroh_ticket.is_none()
        {
            eprintln!(
                "warning: the saved iroh server key is in the build directory. cargo clean or deleting that directory permanently removes the saved key. Use a stable --iroh-secret for production, or bundle a public --iroh-endpoint/--iroh-ticket and set TOPCOAT_IROH_SECRET on the server."
            );
        }
        identity.save(&identity_path)?;
    }
    if iroh {
        let endpoint = args.iroh_endpoint.clone().or_else(|| {
            (automatic_start && args.iroh_ticket.is_none()).then(|| identity.endpoint().to_string())
        });
        let ticket = args.iroh_ticket.as_deref();
        let existing_launcher = out.join("launcher");
        if beside_executable
            && existing_launcher.exists()
            && (!existing_launcher.join(MARKER).is_file() || existing_launcher.is_symlink())
        {
            return Err(format!(
                "{} is not a generated launcher; choose another --out directory",
                existing_launcher.display()
            )
            .into());
        }
        fs::create_dir_all(launcher.join("__topcoat"))?;
        if custom_launcher {
            println!("rendering launcher...");
            run_bundle(&exe, &bytes, Some(launcher.join("__topcoat/assets"))).await?;
            let mut export = Command::new(&exe);
            export.env(
                "TOPCOAT_LAUNCHER_ENDPOINT",
                endpoint.as_deref().unwrap_or_default(),
            );
            export.env("TOPCOAT_LAUNCHER_TICKET", ticket.unwrap_or_default());
            export.args([
                std::ffi::OsStr::new("--topcoat-export-launcher"),
                launcher.as_os_str(),
            ]);
            tokio::time::timeout(
                Duration::from_secs(60),
                run_command(&mut export, "launcher export"),
            )
            .await
            .map_err(|_| "launcher export timed out; call launcher::export_if_requested before server startup and keep the initial view independent of the server")??;
            for path in ["index.html", "404.html", "__topcoat/index.html"] {
                if !launcher.join(path).is_file() {
                    return Err("launcher was not exported; call topcoat::launcher::export_if_requested before server startup".into());
                }
            }
        } else {
            let (_, html) = embedded::LAUNCHER
                .iter()
                .find(|(name, _)| *name == "bootstrap.html")
                .ok_or("missing embedded bootstrap")?;
            for path in ["index.html", "404.html", "__topcoat/index.html"] {
                fs::write(launcher.join(path), html)?;
            }
        }
        fs::write(
            launcher.join("__topcoat/config.json"),
            serde_json::to_vec(&serde_json::json!({ "endpoint": endpoint, "ticket": ticket }))?,
        )?;
        wasm(&target, &launcher).await?;
        for (name, bytes) in embedded::LAUNCHER {
            match *name {
                "topcoat-sw.js" => fs::write(launcher.join(name), bytes)?,
                "bootstrap.js" | "fragment.js" | "settings.js" | "connect.js" | "storage.js"
                | "transport.js" => {
                    fs::write(launcher.join("__topcoat").join(name), bytes)?;
                }
                _ => {}
            }
        }
        fs::write(launcher.join(".nojekyll"), "")?;
        fs::write(launcher.join(MARKER), "1\n")?;
    }
    if beside_executable {
        let mut entries = vec!["assets"];
        if iroh {
            entries.push("launcher");
        }
        publish_entries(&bundle, &out, staging.path(), &entries)?;
    } else {
        fs::write(bundle.join(MARKER), "1\n")?;
        publish(&bundle, &out, staging.path())?;
    }
    println!("server: {}", out.join(executable_name).display());
    if iroh {
        println!("static site: {}", out.join("launcher").display());
        println!("Publish launcher/ at the root of a dedicated HTTPS origin.");
    }
    println!("Run the server executable with its assets directory beside it.");
    Ok(())
}

fn contains_marker(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|window| window == marker)
}

fn build_error(error: BuildError) -> Error {
    match error {
        BuildError::Failed { diagnostics } => format!("build failed\n{diagnostics}").into(),
        error => Box::new(error),
    }
}

async fn wasm(target: &Path, launcher: &Path) -> Result<(), Error> {
    println!("building browser transport...");
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .await?;
    if !installed.status.success() {
        return Err("could not inspect the Rust Wasm target".into());
    }
    if !String::from_utf8_lossy(&installed.stdout)
        .lines()
        .any(|line| line == "wasm32-unknown-unknown")
    {
        run_command(
            Command::new("rustup").args(["target", "add", "wasm32-unknown-unknown"]),
            "install Wasm target",
        )
        .await?;
    }
    let mut hash = DefaultHasher::new();
    (embedded::MANIFEST, embedded::LOCK, embedded::SOURCES).hash(&mut hash);
    let source =
        topcoat_core::cache::cache_dir_in(target, "iroh").join(format!("{:016x}", hash.finish()));
    fs::create_dir_all(source.join("src"))?;
    write_changed(&source.join("Cargo.toml"), embedded::MANIFEST.as_bytes())?;
    if !source.join("Cargo.lock").exists() {
        fs::write(source.join("Cargo.lock"), embedded::LOCK)?;
    }
    for (name, bytes) in embedded::SOURCES {
        write_changed(&source.join("src").join(name), bytes)?;
    }
    // Different standalone source versions share a crate name. Keep their
    // unversioned cdylib outputs separate from each other and the workspace.
    let wasm_target = source.join("target");
    // Cargo prunes unused workspace entries from the embedded lock on the first
    // build. The direct wasm-bindgen version is pinned to the linked generator.
    run_command(
        Command::new("cargo")
            .current_dir(&source)
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "--target-dir",
            ])
            .arg(&wasm_target),
        "browser transport build",
    )
    .await?;
    let input =
        wasm_target.join("wasm32-unknown-unknown/release/topcoat_runtime_iroh_browser.wasm");
    let out = launcher.join("__topcoat");
    tokio::task::spawn_blocking(move || -> Result<(), Error> {
        wasm_bindgen_cli_support::Bindgen::new()
            .input_path(input)
            .web(true)?
            .omit_default_module_path(false)
            .generate(out)?;
        Ok(())
    })
    .await??;
    Ok(())
}

fn write_changed(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if fs::read(path).ok().as_deref() != Some(bytes) {
        fs::write(path, bytes)?;
    }
    Ok(())
}

async fn run_command(command: &mut Command, label: &str) -> Result<(), Error> {
    let status = command.kill_on_drop(true).status().await?;
    if !status.success() {
        return Err(format!("{label} failed ({status})").into());
    }
    Ok(())
}

fn publish(bundle: &Path, out: &Path, staging: &Path) -> Result<(), Error> {
    let previous = staging.join("previous");
    if out.exists() {
        fs::rename(out, &previous)?;
    }
    if let Err(error) = fs::rename(bundle, out) {
        if previous.exists() {
            fs::rename(previous, out)?;
        }
        return Err(error.into());
    }
    Ok(())
}

// Replace only generated directories when publishing beside Cargo's executable.
// Cargo owns the rest of the profile directory.
fn publish_entries(
    bundle: &Path,
    out: &Path,
    staging: &Path,
    entries: &[&str],
) -> Result<(), Error> {
    let previous = staging.join("previous");
    fs::create_dir(&previous)?;
    let mut published = Vec::new();
    for name in entries {
        let destination = out.join(name);
        let backup = previous.join(name);
        let result = (|| -> std::io::Result<()> {
            if destination.exists() {
                fs::rename(&destination, &backup)?;
            }
            fs::rename(bundle.join(name), &destination)
        })();
        if let Err(error) = result {
            if backup.exists() {
                fs::rename(&backup, &destination)?;
            }
            for name in published.into_iter().rev() {
                fs::rename(out.join(name), bundle.join(name))?;
                let backup = previous.join(name);
                if backup.exists() {
                    fs::rename(backup, out.join(name))?;
                }
            }
            return Err(error.into());
        }
        published.push(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_profile_publish_restores_assets_and_preserves_cargo_files() {
        let temporary = tempfile::tempdir().unwrap();
        let out = temporary.path().join("release");
        let bundle = temporary.path().join("bundle");
        fs::create_dir_all(out.join("assets")).unwrap();
        fs::create_dir_all(out.join("launcher")).unwrap();
        fs::create_dir_all(bundle.join("assets")).unwrap();
        fs::write(out.join("app"), "executable").unwrap();
        fs::write(out.join("assets/old"), "previous assets").unwrap();
        fs::write(out.join("launcher/index.html"), "previous launcher").unwrap();
        fs::write(bundle.join("assets/new"), "new assets").unwrap();
        assert!(publish_entries(&bundle, &out, temporary.path(), &["assets", "launcher"]).is_err());
        assert_eq!(fs::read_to_string(out.join("app")).unwrap(), "executable");
        assert_eq!(
            fs::read_to_string(out.join("assets/old")).unwrap(),
            "previous assets"
        );
        assert_eq!(
            fs::read_to_string(out.join("launcher/index.html")).unwrap(),
            "previous launcher"
        );
        assert!(!out.join("assets/new").exists());
    }

    #[test]
    fn failed_publish_restores_the_previous_bundle() {
        let temporary = tempfile::tempdir().unwrap();
        let out = temporary.path().join("output");
        fs::create_dir(&out).unwrap();
        fs::write(out.join("index.html"), "previous build").unwrap();
        assert!(publish(&temporary.path().join("missing"), &out, temporary.path()).is_err());
        assert_eq!(
            fs::read_to_string(out.join("index.html")).unwrap(),
            "previous build"
        );
    }
}
