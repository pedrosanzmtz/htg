//! SRTM tile download functionality.
//!
//! This module provides functionality to download SRTM tiles from remote servers.
//! It is only available when the `download` feature is enabled.
//!
//! # Data Sources
//!
//! SRTM data is available from several sources:
//!
//! - **NASA Earthdata**: High quality, but requires authentication
//! - **CGIAR-CSI**: Processed SRTM data, free access
//! - **ViewFinderPanoramas**: Community-curated, includes void-filled data
//!
//! This module supports configurable data sources via URL templates.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use reqwest::blocking::Client;

use crate::error::{Result, SrtmError};
use crate::filename::lat_lon_to_filename;

/// Default timeout for HTTP requests in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Known SRTM data sources.
#[derive(Debug, Clone)]
pub enum SrtmSource {
    /// NASA Earthdata SRTM (requires authentication).
    /// URL pattern: `https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/2000.02.11/{filename}.SRTMGL1.hgt.zip`
    NasaEarthdata {
        /// NASA Earthdata username
        username: String,
        /// NASA Earthdata password
        password: String,
    },

    /// Custom URL template.
    /// Use `{filename}` as placeholder for the tile name (e.g., "N35E138").
    /// Use `{lat_prefix}`, `{lat}`, `{lon_prefix}`, `{lon}` for individual components.
    ///
    /// Examples:
    /// - `https://example.com/srtm/{filename}.hgt.gz`
    /// - `https://example.com/{lat_prefix}{lat}/{filename}.hgt`
    Custom {
        /// URL template with placeholders
        url_template: String,
        /// Whether the file is gzip compressed
        is_gzipped: bool,
    },
}

impl Default for SrtmSource {
    fn default() -> Self {
        // Default to a custom template that users must configure
        SrtmSource::Custom {
            url_template: String::new(),
            is_gzipped: false,
        }
    }
}

/// Configuration for downloading SRTM tiles.
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// The data source to download from.
    pub source: SrtmSource,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Number of retry attempts on failure.
    pub max_retries: u32,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            source: SrtmSource::default(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: 3,
        }
    }
}

impl DownloadConfig {
    /// Create a new download configuration with a custom URL template.
    ///
    /// # Arguments
    ///
    /// * `url_template` - URL template with `{filename}` placeholder
    /// * `is_gzipped` - Whether the downloaded file is gzip compressed
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::download::DownloadConfig;
    ///
    /// let config = DownloadConfig::with_url_template(
    ///     "https://example.com/srtm/{filename}.hgt.gz",
    ///     true,
    /// );
    /// ```
    pub fn with_url_template(url_template: impl Into<String>, is_gzipped: bool) -> Self {
        Self {
            source: SrtmSource::Custom {
                url_template: url_template.into(),
                is_gzipped,
            },
            ..Default::default()
        }
    }

    /// Create a configuration for NASA Earthdata.
    ///
    /// Requires a NASA Earthdata account: <https://urs.earthdata.nasa.gov/>
    pub fn nasa_earthdata(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            source: SrtmSource::NasaEarthdata {
                username: username.into(),
                password: password.into(),
            },
            ..Default::default()
        }
    }

    /// Set the request timeout.
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set the maximum number of retry attempts.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }
}

/// SRTM tile downloader.
pub struct Downloader {
    client: Client,
    config: DownloadConfig,
}

impl Downloader {
    /// Create a new downloader with the given configuration.
    pub fn new(config: DownloadConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| SrtmError::DownloadFailed {
                filename: String::new(),
                reason: format!("Failed to create HTTP client: {}", e),
            })?;

        Ok(Self { client, config })
    }

    /// Download a tile for the given coordinates.
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees
    /// * `lon` - Longitude in decimal degrees
    /// * `dest_dir` - Directory to save the downloaded file
    ///
    /// # Returns
    ///
    /// The path to the downloaded `.hgt` file.
    pub fn download_tile(&self, lat: f64, lon: f64, dest_dir: &Path) -> Result<std::path::PathBuf> {
        let filename = lat_lon_to_filename(lat, lon);
        self.download_tile_by_name(&filename, dest_dir)
    }

    /// Download a tile by its filename.
    ///
    /// # Arguments
    ///
    /// * `filename` - The tile filename (e.g., "N35E138.hgt")
    /// * `dest_dir` - Directory to save the downloaded file
    pub fn download_tile_by_name(
        &self,
        filename: &str,
        dest_dir: &Path,
    ) -> Result<std::path::PathBuf> {
        // Remove .hgt extension if present for URL building
        let base_name = filename.strip_suffix(".hgt").unwrap_or(filename);

        let url = self.build_url(base_name)?;
        let dest_path = dest_dir.join(format!("{}.hgt", base_name));

        // Skip if file already exists
        if dest_path.exists() {
            return Ok(dest_path);
        }

        // Ensure destination directory exists
        fs::create_dir_all(dest_dir)?;

        // Download with retries
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                // Brief delay before retry
                std::thread::sleep(std::time::Duration::from_millis(500 * attempt as u64));
            }

            match self.do_download(&url, &dest_path) {
                Ok(()) => return Ok(dest_path),
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| SrtmError::DownloadFailed {
            filename: filename.to_string(),
            reason: "Unknown error".to_string(),
        }))
    }

    /// Build the download URL for a tile.
    fn build_url(&self, base_name: &str) -> Result<String> {
        // Parse components from filename (e.g., "N35E138")
        let (lat_prefix, lat, lon_prefix, lon) = parse_filename_components(base_name)?;

        match &self.config.source {
            SrtmSource::NasaEarthdata { .. } => {
                // NASA Earthdata URL pattern
                Ok(format!(
                    "https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/2000.02.11/{}.SRTMGL1.hgt.zip",
                    base_name
                ))
            }
            SrtmSource::Custom { url_template, .. } => {
                if url_template.is_empty() {
                    return Err(SrtmError::DownloadFailed {
                        filename: format!("{}.hgt", base_name),
                        reason: "No download URL template configured".to_string(),
                    });
                }

                let url = url_template
                    .replace("{filename}", base_name)
                    .replace("{lat_prefix}", lat_prefix)
                    .replace("{lat}", lat)
                    .replace("{lon_prefix}", lon_prefix)
                    .replace("{lon}", lon);

                Ok(url)
            }
        }
    }

    /// Perform the actual download.
    fn do_download(&self, url: &str, dest_path: &Path) -> Result<()> {
        let mut request = self.client.get(url);

        // Add authentication if needed
        if let SrtmSource::NasaEarthdata { username, password } = &self.config.source {
            request = request.basic_auth(username, Some(password));
        }

        let response = request.send()?;

        if !response.status().is_success() {
            return Err(SrtmError::DownloadFailed {
                filename: dest_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                reason: format!("HTTP {}", response.status()),
            });
        }

        let bytes = response.bytes()?;

        // Check if we need to decompress
        let is_gzipped = match &self.config.source {
            SrtmSource::Custom { is_gzipped, .. } => *is_gzipped,
            SrtmSource::NasaEarthdata { .. } => false, // NASA uses .zip, needs different handling
        };

        if is_gzipped {
            // Decompress gzip data
            let mut decoder = GzDecoder::new(&bytes[..]);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| SrtmError::DownloadFailed {
                    filename: dest_path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    reason: format!("Failed to decompress: {}", e),
                })?;

            let mut file = File::create(dest_path)?;
            file.write_all(&decompressed)?;
        } else {
            // Write raw bytes
            let mut file = File::create(dest_path)?;
            io::copy(&mut &bytes[..], &mut file)?;
        }

        Ok(())
    }
}

/// Parse filename components (e.g., "N35E138" -> ("N", "35", "E", "138")).
fn parse_filename_components(base_name: &str) -> Result<(&str, &str, &str, &str)> {
    if base_name.len() != 7 {
        return Err(SrtmError::DownloadFailed {
            filename: format!("{}.hgt", base_name),
            reason: "Invalid filename format".to_string(),
        });
    }

    let lat_prefix = &base_name[0..1];
    let lat = &base_name[1..3];
    let lon_prefix = &base_name[3..4];
    let lon = &base_name[4..7];

    Ok((lat_prefix, lat, lon_prefix, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filename_components() {
        let (lat_p, lat, lon_p, lon) = parse_filename_components("N35E138").unwrap();
        assert_eq!(lat_p, "N");
        assert_eq!(lat, "35");
        assert_eq!(lon_p, "E");
        assert_eq!(lon, "138");

        let (lat_p, lat, lon_p, lon) = parse_filename_components("S12W077").unwrap();
        assert_eq!(lat_p, "S");
        assert_eq!(lat, "12");
        assert_eq!(lon_p, "W");
        assert_eq!(lon, "077");
    }

    #[test]
    fn test_build_url_custom() {
        let config = DownloadConfig::with_url_template(
            "https://example.com/srtm/{lat_prefix}{lat}/{filename}.hgt.gz",
            true,
        );
        let downloader = Downloader::new(config).unwrap();
        let url = downloader.build_url("N35E138").unwrap();
        assert_eq!(url, "https://example.com/srtm/N35/N35E138.hgt.gz");
    }

    #[test]
    fn test_empty_url_template() {
        let config = DownloadConfig::default();
        let downloader = Downloader::new(config).unwrap();
        let result = downloader.build_url("N35E138");
        assert!(result.is_err());
    }

    #[test]
    fn test_download_config_builder() {
        let config = DownloadConfig::with_url_template("https://example.com/{filename}.hgt", false)
            .with_timeout(60)
            .with_max_retries(5);

        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_retries, 5);
    }
}
