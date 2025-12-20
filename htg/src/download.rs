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
use std::io::{Cursor, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use zip::ZipArchive;

use crate::error::{Result, SrtmError};
use crate::filename::lat_lon_to_filename;

/// Compression format for downloaded SRTM files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// No compression - raw .hgt file
    #[default]
    None,
    /// Gzip compression (.hgt.gz)
    Gzip,
    /// ZIP archive (.hgt.zip)
    Zip,
}

impl Compression {
    /// Detect compression format from a URL or filename.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use htg::download::Compression;
    ///
    /// assert_eq!(Compression::from_url("file.hgt.gz"), Compression::Gzip);
    /// assert_eq!(Compression::from_url("file.hgt.zip"), Compression::Zip);
    /// assert_eq!(Compression::from_url("file.hgt"), Compression::None);
    /// ```
    pub fn from_url(url: &str) -> Self {
        let lower = url.to_lowercase();
        if lower.ends_with(".gz") {
            Compression::Gzip
        } else if lower.ends_with(".zip") {
            Compression::Zip
        } else {
            Compression::None
        }
    }
}

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
    /// - `https://example.com/srtm/{filename}.hgt.zip`
    /// - `https://example.com/{lat_prefix}{lat}/{filename}.hgt`
    Custom {
        /// URL template with placeholders
        url_template: String,
        /// Compression format of the downloaded file
        compression: Compression,
    },
}

impl Default for SrtmSource {
    fn default() -> Self {
        // Default to a custom template that users must configure
        SrtmSource::Custom {
            url_template: String::new(),
            compression: Compression::None,
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
    /// Compression is auto-detected from the URL extension:
    /// - `.gz` → Gzip
    /// - `.zip` → ZIP
    /// - otherwise → None
    ///
    /// # Arguments
    ///
    /// * `url_template` - URL template with `{filename}` placeholder
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::download::DownloadConfig;
    ///
    /// // Compression auto-detected from .gz extension
    /// let config = DownloadConfig::with_url_template(
    ///     "https://example.com/srtm/{filename}.hgt.gz",
    /// );
    ///
    /// // ZIP compression auto-detected
    /// let config = DownloadConfig::with_url_template(
    ///     "https://example.com/srtm/{filename}.hgt.zip",
    /// );
    /// ```
    pub fn with_url_template(url_template: impl Into<String>) -> Self {
        let template = url_template.into();
        let compression = Compression::from_url(&template);
        Self {
            source: SrtmSource::Custom {
                url_template: template,
                compression,
            },
            ..Default::default()
        }
    }

    /// Create a new download configuration with explicit compression setting.
    ///
    /// # Arguments
    ///
    /// * `url_template` - URL template with `{filename}` placeholder
    /// * `compression` - Compression format of the downloaded file
    pub fn with_url_template_and_compression(
        url_template: impl Into<String>,
        compression: Compression,
    ) -> Self {
        Self {
            source: SrtmSource::Custom {
                url_template: url_template.into(),
                compression,
            },
            ..Default::default()
        }
    }

    /// Create a new download configuration with a custom URL template.
    ///
    /// **Deprecated:** Use `with_url_template` (auto-detects compression) or
    /// `with_url_template_and_compression` instead.
    #[deprecated(
        since = "0.2.0",
        note = "Use with_url_template (auto-detects) or with_url_template_and_compression"
    )]
    pub fn with_url_template_gzipped(url_template: impl Into<String>, is_gzipped: bool) -> Self {
        let compression = if is_gzipped {
            Compression::Gzip
        } else {
            Compression::None
        };
        Self::with_url_template_and_compression(url_template, compression)
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

        // Determine compression format
        let compression = match &self.config.source {
            SrtmSource::Custom { compression, .. } => *compression,
            SrtmSource::NasaEarthdata { .. } => Compression::Zip,
        };

        let filename = dest_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let decompressed = match compression {
            Compression::None => bytes.to_vec(),
            Compression::Gzip => {
                let mut decoder = GzDecoder::new(&bytes[..]);
                let mut data = Vec::new();
                decoder
                    .read_to_end(&mut data)
                    .map_err(|e| SrtmError::DownloadFailed {
                        filename: filename.clone(),
                        reason: format!("Failed to decompress gzip: {}", e),
                    })?;
                data
            }
            Compression::Zip => Self::extract_hgt_from_zip(&bytes, &filename)?,
        };

        let mut file = File::create(dest_path)?;
        file.write_all(&decompressed)?;

        Ok(())
    }

    /// Extract an .hgt file from a ZIP archive.
    ///
    /// Searches the archive for a file ending in ".hgt" (case-insensitive)
    /// and returns its contents.
    fn extract_hgt_from_zip(data: &[u8], filename: &str) -> Result<Vec<u8>> {
        let cursor = Cursor::new(data);
        let mut archive = ZipArchive::new(cursor).map_err(|e| SrtmError::DownloadFailed {
            filename: filename.to_string(),
            reason: format!("Failed to read ZIP archive: {}", e),
        })?;

        // Search for an .hgt file in the archive
        for i in 0..archive.len() {
            let mut zip_file = archive.by_index(i).map_err(|e| SrtmError::DownloadFailed {
                filename: filename.to_string(),
                reason: format!("Failed to read ZIP entry: {}", e),
            })?;

            let name = zip_file.name().to_lowercase();
            if name.ends_with(".hgt") {
                let mut contents = Vec::new();
                zip_file
                    .read_to_end(&mut contents)
                    .map_err(|e| SrtmError::DownloadFailed {
                        filename: filename.to_string(),
                        reason: format!("Failed to extract .hgt from ZIP: {}", e),
                    })?;
                return Ok(contents);
            }
        }

        Err(SrtmError::DownloadFailed {
            filename: filename.to_string(),
            reason: "No .hgt file found in ZIP archive".to_string(),
        })
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
        let config = DownloadConfig::with_url_template("https://example.com/{filename}.hgt")
            .with_timeout(60)
            .with_max_retries(5);

        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_retries, 5);
    }

    #[test]
    fn test_compression_from_url() {
        assert_eq!(Compression::from_url("file.hgt"), Compression::None);
        assert_eq!(Compression::from_url("file.hgt.gz"), Compression::Gzip);
        assert_eq!(Compression::from_url("file.hgt.zip"), Compression::Zip);
        assert_eq!(Compression::from_url("FILE.HGT.GZ"), Compression::Gzip);
        assert_eq!(Compression::from_url("FILE.HGT.ZIP"), Compression::Zip);
        assert_eq!(
            Compression::from_url("https://example.com/srtm/N35E138.hgt.zip"),
            Compression::Zip
        );
    }

    #[test]
    fn test_compression_auto_detect() {
        let config = DownloadConfig::with_url_template("https://example.com/{filename}.hgt.gz");
        if let SrtmSource::Custom { compression, .. } = config.source {
            assert_eq!(compression, Compression::Gzip);
        } else {
            panic!("Expected Custom source");
        }

        let config = DownloadConfig::with_url_template("https://example.com/{filename}.hgt.zip");
        if let SrtmSource::Custom { compression, .. } = config.source {
            assert_eq!(compression, Compression::Zip);
        } else {
            panic!("Expected Custom source");
        }

        let config = DownloadConfig::with_url_template("https://example.com/{filename}.hgt");
        if let SrtmSource::Custom { compression, .. } = config.source {
            assert_eq!(compression, Compression::None);
        } else {
            panic!("Expected Custom source");
        }
    }

    #[test]
    fn test_extract_hgt_from_zip() {
        // Create a minimal ZIP file with a fake .hgt file
        let mut zip_buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut zip_buffer));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("N35E138.hgt", options).unwrap();
            // Write some fake HGT data (just a few bytes for testing)
            zip.write_all(&[0u8; 100]).unwrap();
            zip.finish().unwrap();
        }

        let result = Downloader::extract_hgt_from_zip(&zip_buffer, "test.hgt");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 100);
    }

    #[test]
    fn test_extract_hgt_from_zip_no_hgt_file() {
        // Create a ZIP file without an .hgt file
        let mut zip_buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut zip_buffer));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("readme.txt", options).unwrap();
            zip.write_all(b"Not an HGT file").unwrap();
            zip.finish().unwrap();
        }

        let result = Downloader::extract_hgt_from_zip(&zip_buffer, "test.hgt");
        assert!(result.is_err());
    }
}
