//! Runtime typeshed download and caching
//!
//! Instead of vendoring typeshed in the repository, we download it on first use
//! and cache it in the user's cache directory. This keeps the repo clean and
//! makes updates easier.

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use path_slash::PathExt;
use ruff_db::vendored::VendoredFileSystem;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tar::Archive;
use zip::write::{FileOptions, ZipWriter};
use zip::CompressionMethod;

/// GitHub URL for typeshed main branch tarball
const TYPESHED_URL: &str = "https://github.com/python/typeshed/archive/main.tar.gz";

/// Version/commit marker file to track what's cached
const VERSION_FILE: &str = "version.txt";
const CURRENT_VERSION: &str = "main"; // or pin to a specific commit

/// Lazily initialized vendored filesystem with cached typeshed stubs
static VENDORED_TYPESHED: LazyLock<VendoredFileSystem> = LazyLock::new(|| {
    let typeshed_path = find_or_download_typeshed().expect("Failed to locate or download typeshed");

    // Zip the directory into memory (happens once)
    let zip_bytes = zip_directory(&typeshed_path).expect("Failed to zip typeshed");

    VendoredFileSystem::new(zip_bytes).expect("Failed to create VendoredFileSystem from typeshed")
});

/// Get the vendored typeshed filesystem
///
/// This returns a reference to a lazily-initialized VendoredFileSystem
/// that contains all the typeshed stubs. On first call, it will download
/// and cache typeshed if not already present.
pub fn vendored_typeshed() -> &'static VendoredFileSystem {
    &VENDORED_TYPESHED
}

/// Find or download typeshed, returning the path to the stdlib directory
fn find_or_download_typeshed() -> Result<PathBuf> {
    // 1. Check environment variable for override
    if let Ok(path) = std::env::var("TYPESHED_PATH") {
        let typeshed_path = PathBuf::from(path);
        if typeshed_path.join("stdlib").exists() {
            return Ok(typeshed_path);
        }
        eprintln!(
            "⚠️  TYPESHED_PATH set but stdlib not found at: {}",
            typeshed_path.display()
        );
    }

    // 2. Use cache directory
    let cache_dir = get_cache_dir()?;
    let typeshed_dir = cache_dir.join("typeshed");
    let stdlib_dir = typeshed_dir.join("stdlib");
    let version_file = typeshed_dir.join(VERSION_FILE);

    // Check if cache is valid
    let needs_download = !stdlib_dir.exists()
        || !version_file.exists()
        || fs::read_to_string(&version_file).unwrap_or_default().trim() != CURRENT_VERSION;

    if needs_download {
        eprintln!("📦 Downloading typeshed (~7MB, one-time setup)...");
        download_and_extract_typeshed(&typeshed_dir)?;

        // Write version marker
        fs::write(&version_file, CURRENT_VERSION)?;

        eprintln!("✓ Typeshed cached at: {}", typeshed_dir.display());
    }

    Ok(typeshed_dir)
}

/// Get the cache directory for portlang
fn get_cache_dir() -> Result<PathBuf> {
    let cache = dirs::cache_dir()
        .ok_or_else(|| anyhow!("Could not determine cache directory"))?
        .join("portlang");

    fs::create_dir_all(&cache).context("Failed to create cache directory")?;

    Ok(cache)
}

/// Download typeshed from GitHub and extract to target directory
fn download_and_extract_typeshed(target_dir: &Path) -> Result<()> {
    // Clean target directory if it exists
    if target_dir.exists() {
        fs::remove_dir_all(target_dir)?;
    }
    fs::create_dir_all(target_dir)?;

    // Download tarball
    let response = reqwest::blocking::get(TYPESHED_URL).context("Failed to download typeshed")?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to download typeshed: HTTP {}",
            response.status()
        ));
    }

    // Decompress and extract
    let gz = GzDecoder::new(BufReader::new(response));
    let mut archive = Archive::new(gz);

    // Extract only the stdlib directory
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Skip non-stdlib files
        // Path format: typeshed-main/stdlib/...
        let components: Vec<_> = path.components().collect();
        if components.len() < 2 {
            continue;
        }

        // Check if this is in the stdlib directory
        let component_str = components[1].as_os_str().to_string_lossy();
        if component_str != "stdlib" {
            continue;
        }

        // Build target path (removing the typeshed-main/ prefix)
        let relative_path: PathBuf = components[1..].iter().collect();
        let target_path = target_dir.join(&relative_path);

        // Create parent directory
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Extract file
        if entry.header().entry_type().is_file() {
            entry.unpack(&target_path)?;
        } else if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target_path)?;
        }
    }

    // Verify stdlib exists
    if !target_dir.join("stdlib").exists() {
        return Err(anyhow!("Failed to extract stdlib from typeshed archive"));
    }

    Ok(())
}

/// Zip a directory into a byte vector (no compression, for VendoredFileSystem)
fn zip_directory(dir: &Path) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buffer));

    // Use stored (no compression) to match Ruff's requirements
    let method = CompressionMethod::Stored;
    let options = FileOptions::<()>::default()
        .compression_method(method)
        .unix_permissions(0o644);

    // Walk the directory and add files
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        let path = entry.path();

        let relative_path = path
            .strip_prefix(dir)
            .context("Failed to strip prefix")?
            .to_slash()
            .ok_or_else(|| anyhow!("Non-UTF8 path"))?;

        if path.is_file() {
            zip.start_file(&*relative_path, options)?;
            let mut file = File::open(path)?;
            std::io::copy(&mut file, &mut zip)?;
        } else if !relative_path.is_empty() {
            zip.add_directory(&*relative_path, options)?;
        }
    }

    zip.finish()?;

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vendored_typeshed_loads() {
        // This will trigger download on first run
        let fs = vendored_typeshed();

        // Just verify it doesn't panic and contains something
        assert!(format!("{:?}", fs).contains("VendoredFileSystem"));
    }

    #[test]
    fn test_cache_dir_creation() {
        let cache = get_cache_dir().expect("Failed to get cache dir");
        assert!(cache.exists());
        assert!(cache.ends_with("portlang"));
    }
}
