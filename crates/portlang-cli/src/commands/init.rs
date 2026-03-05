use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

const CONTAINER_RELEASE_URL: &str = "https://github.com/apple/container/releases/latest";
const GITHUB_API_RELEASE_URL: &str = "https://api.github.com/repos/apple/container/releases/latest";

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

#[derive(Debug)]
struct SystemInfo {
    os: String,
    version: String,
    is_macos: bool,
}

#[derive(Debug)]
enum ContainerStatus {
    NotInstalled,
    Installed { version: String, running: bool },
}

impl SystemInfo {
    fn detect() -> Result<Self> {
        let os = std::env::consts::OS;

        if os == "macos" {
            let output = Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .context("Failed to get macOS version")?;

            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

            Ok(Self {
                os: "macOS".to_string(),
                version,
                is_macos: true,
            })
        } else {
            Ok(Self {
                os: os.to_string(),
                version: String::new(),
                is_macos: false,
            })
        }
    }
}

impl ContainerStatus {
    fn check() -> Self {
        // Check if container binary exists
        let version_output = Command::new("container").arg("--version").output();

        match version_output {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

                // Check if system is running
                let system_status = Command::new("container")
                    .args(["system", "status"])
                    .output();

                let running = system_status.map(|s| s.status.success()).unwrap_or(false);

                ContainerStatus::Installed { version, running }
            }
            _ => ContainerStatus::NotInstalled,
        }
    }
}

pub fn init_command() -> Result<()> {
    println!("🔍 Checking portlang environment...\n");

    // Detect system
    let system = SystemInfo::detect()?;
    println!("Operating System: {} {}", system.os, system.version);

    // Check container support
    if !system.is_macos {
        println!("\n⚠️  Apple Container is only available on macOS.");
        println!(
            "   On {}, portlang will use the dispatch sandbox (non-containerized).",
            system.os
        );
        println!("\n✓ portlang is ready to use with dispatch sandbox.");
        return Ok(());
    }

    // Check container installation on macOS
    println!("\n🐳 Checking Apple Container installation...");
    let container_status = ContainerStatus::check();

    match container_status {
        ContainerStatus::NotInstalled => {
            println!("\n❌ Apple Container is not installed.\n");
            println!("To enable containerized sandboxing, install Apple Container:");
            println!("\n📦 Installation Steps:");
            println!("   1. Download the latest release:");
            println!("      {}", CONTAINER_RELEASE_URL);
            println!("\n   2. Run the automated installer:");
            println!("      portlang init --install");
            println!("\n   3. Or download and install manually:");
            println!("      • Download 'container-*-installer-signed.pkg'");
            println!("      • Double-click to install (requires admin password)");
            println!("      • Run: container system start");
            println!("\n⚠️  Currently using dispatch sandbox (non-containerized).");
        }

        ContainerStatus::Installed { version, running } => {
            println!("✓ Apple Container installed: {}", version);

            if running {
                println!("✓ Container system is running");
                println!("\n🎉 portlang is fully configured!");
                println!("\n   All fields will run in containerized sandboxes.");
            } else {
                println!("\n⚠️  Container system is not running.");
                println!("\n🚀 Start the container system:");
                println!("   container system start");
                println!("\nOr run:");
                println!("   portlang init --start");

                return Err(anyhow::anyhow!(
                    "Container system is not running. Please start it with: container system start"
                ));
            }
        }
    }

    println!();
    Ok(())
}

pub async fn init_install_command() -> Result<()> {
    println!("📦 Installing Apple Container...\n");

    // Check system
    let system = SystemInfo::detect()?;

    if !system.is_macos {
        return Err(anyhow::anyhow!(
            "Apple Container is only available on macOS"
        ));
    }

    // Check if already installed
    if matches!(ContainerStatus::check(), ContainerStatus::Installed { .. }) {
        println!("✓ Apple Container is already installed");
        return Ok(());
    }

    // Download installer
    println!("Fetching latest Apple Container release from GitHub...");

    let tmp_dir = std::env::temp_dir();
    let installer_path = tmp_dir.join("container-installer-signed.pkg");

    // Fetch latest release info from GitHub API
    let client = reqwest::Client::builder()
        .user_agent("portlang-cli")
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(GITHUB_API_RELEASE_URL)
        .send()
        .await
        .context("Failed to fetch release information from GitHub")?;

    if !response.status().is_success() {
        println!("⚠️  Could not fetch latest version from GitHub API.");
        println!("\nPlease download manually from: {}", CONTAINER_RELEASE_URL);
        return Err(anyhow::anyhow!(
            "Failed to fetch release info: HTTP {}",
            response.status()
        ));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .context("Failed to parse release information")?;

    // Find the installer package
    let installer_asset = release
        .assets
        .iter()
        .find(|asset| asset.name.contains("installer-signed.pkg"))
        .ok_or_else(|| anyhow::anyhow!("Could not find installer package in release assets"))?;

    let download_url = &installer_asset.browser_download_url;
    println!("Found installer: {}", installer_asset.name);
    println!("Downloading from: {}", download_url);

    // Download the installer
    let installer_bytes = client
        .get(download_url)
        .send()
        .await
        .context("Failed to download installer")?
        .bytes()
        .await
        .context("Failed to read installer data")?;

    // Write to temp file
    std::fs::write(&installer_path, &installer_bytes)
        .context("Failed to write installer to disk")?;

    println!("✓ Downloaded to: {}", installer_path.display());

    // Open installer
    println!("\n🚀 Opening installer...");
    println!("   You will be prompted for your administrator password.");

    Command::new("open")
        .arg(&installer_path)
        .status()
        .context("Failed to open installer")?;

    println!("\n✓ Installer opened. Complete the installation, then run:");
    println!("   portlang init --start");

    Ok(())
}

pub fn init_start_command() -> Result<()> {
    println!("🚀 Starting Apple Container system...\n");

    // Check if installed
    match ContainerStatus::check() {
        ContainerStatus::NotInstalled => {
            return Err(anyhow::anyhow!(
                "Apple Container is not installed. Run: cargo run -p portlang-cli -- init --install"
            ));
        }
        ContainerStatus::Installed {
            version: _,
            running: true,
        } => {
            println!("✓ Container system is already running");
            return Ok(());
        }
        ContainerStatus::Installed {
            version,
            running: false,
        } => {
            println!("Starting container system (version {})...", version);

            let status = Command::new("container")
                .args(["system", "start"])
                .status()
                .context("Failed to start container system")?;

            if status.success() {
                println!("\n✓ Container system started successfully!");
                println!("\n🎉 portlang is ready to use containerized sandboxing!");
            } else {
                return Err(anyhow::anyhow!("Failed to start container system"));
            }
        }
    }

    Ok(())
}
