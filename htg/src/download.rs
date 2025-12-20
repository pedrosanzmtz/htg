//! SRTM tile download functionality.
//!
//! This module provides functionality to download SRTM tiles from remote servers.
//! It is only available when the `download` feature is enabled.
//!
//! # Data Sources
//!
//! SRTM data is available from several sources:
//!
//! - **ArduPilot Terrain Server**: Free access, organized by continent
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
    /// ArduPilot terrain server - SRTM1 (1 arc-second, ~30m resolution).
    /// URL pattern: `https://terrain.ardupilot.org/SRTM1/{filename}.hgt.zip`
    ///
    /// Flat directory structure (no continent subdirectories).
    /// Higher resolution (~25MB per tile) - recommended for accuracy.
    ArduPilotSrtm1,

    /// ArduPilot terrain server - SRTM3 (3 arc-second, ~90m resolution).
    /// URL pattern: `https://terrain.ardupilot.org/SRTM3/{continent}/{filename}.hgt.zip`
    ///
    /// Files are organized by continent subdirectories:
    /// - North_America, South_America, Eurasia, Africa, Australia
    ///
    /// Lower resolution (~2.8MB per tile) - faster downloads, less storage.
    ArduPilotSrtm3,

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
    /// Use `{continent}` for ArduPilot-style continent subdirectories.
    ///
    /// Examples:
    /// - `https://example.com/srtm/{filename}.hgt.gz`
    /// - `https://example.com/srtm/{filename}.hgt.zip`
    /// - `https://example.com/{lat_prefix}{lat}/{filename}.hgt`
    /// - `https://example.com/{continent}/{filename}.hgt.zip`
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

    /// Create a configuration for ArduPilot terrain server (SRTM1 - high resolution).
    ///
    /// Uses <https://terrain.ardupilot.org/SRTM1/{continent}/{filename}.hgt.zip>
    ///
    /// This is a free, public SRTM data source that doesn't require authentication.
    /// Tiles are organized by continent subdirectories.
    ///
    /// SRTM1 provides 1 arc-second (~30m) resolution with ~25MB per tile.
    /// For smaller downloads, use [`ardupilot_srtm3`](Self::ardupilot_srtm3).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::download::DownloadConfig;
    ///
    /// let config = DownloadConfig::ardupilot();
    /// // Downloads from https://terrain.ardupilot.org/SRTM1/Eurasia/N35E138.hgt.zip
    /// ```
    pub fn ardupilot() -> Self {
        Self::ardupilot_srtm1()
    }

    /// Create a configuration for ArduPilot terrain server (SRTM1 - high resolution).
    ///
    /// Uses <https://terrain.ardupilot.org/SRTM1/{continent}/{filename}.hgt.zip>
    ///
    /// SRTM1 provides 1 arc-second (~30m) resolution with ~25MB per tile.
    pub fn ardupilot_srtm1() -> Self {
        Self {
            source: SrtmSource::ArduPilotSrtm1,
            ..Default::default()
        }
    }

    /// Create a configuration for ArduPilot terrain server (SRTM3 - lower resolution).
    ///
    /// Uses <https://terrain.ardupilot.org/SRTM3/{continent}/{filename}.hgt.zip>
    ///
    /// SRTM3 provides 3 arc-second (~90m) resolution with ~2.8MB per tile.
    /// Faster downloads and less storage, but lower accuracy.
    pub fn ardupilot_srtm3() -> Self {
        Self {
            source: SrtmSource::ArduPilotSrtm3,
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
        let (lat_prefix, lat_str, lon_prefix, lon_str) = parse_filename_components(base_name)?;

        match &self.config.source {
            SrtmSource::ArduPilotSrtm1 => {
                // SRTM1 uses flat structure (no continent subdirectories)
                Ok(format!(
                    "https://terrain.ardupilot.org/SRTM1/{}.hgt.zip",
                    base_name
                ))
            }
            SrtmSource::ArduPilotSrtm3 => {
                // SRTM3 uses continent subdirectories
                let lat = parse_coord_from_components(lat_prefix, lat_str);
                let lon = parse_coord_from_components(lon_prefix, lon_str);

                let continent =
                    coords_to_continent(lat, lon).ok_or_else(|| SrtmError::DownloadFailed {
                        filename: format!("{}.hgt", base_name),
                        reason: format!(
                            "Coordinates ({}, {}) do not map to a known continent",
                            lat, lon
                        ),
                    })?;

                Ok(format!(
                    "https://terrain.ardupilot.org/SRTM3/{}/{}.hgt.zip",
                    continent, base_name
                ))
            }
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

                // Compute coordinates for {continent} placeholder if present
                let continent = if url_template.contains("{continent}") {
                    let lat = parse_coord_from_components(lat_prefix, lat_str);
                    let lon = parse_coord_from_components(lon_prefix, lon_str);
                    coords_to_continent(lat, lon).unwrap_or("")
                } else {
                    ""
                };

                let url = url_template
                    .replace("{filename}", base_name)
                    .replace("{lat_prefix}", lat_prefix)
                    .replace("{lat}", lat_str)
                    .replace("{lon_prefix}", lon_prefix)
                    .replace("{lon}", lon_str)
                    .replace("{continent}", continent);

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
            SrtmSource::ArduPilotSrtm1
            | SrtmSource::ArduPilotSrtm3
            | SrtmSource::NasaEarthdata { .. } => Compression::Zip,
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

/// Map coordinates to ArduPilot continent subdirectory.
///
/// Returns the continent name used in ArduPilot's SRTM directory structure,
/// or `None` if the coordinates don't map to a known continent.
///
/// The mapping is based on approximate geographic boundaries:
/// - North_America: 15°N to 60°N, 170°W to 50°W
/// - South_America: 60°S to 15°N, 90°W to 30°W
/// - Australia: 50°S to 10°S, 110°E to 180°E
/// - Africa: 35°S to 35°N, 20°W to 55°E
/// - Eurasia: 0°N to 60°N, 15°W to 180°E (fallback for overlapping regions)
///
/// Note: Some regions may overlap. Priority order is used to resolve conflicts.
pub fn coords_to_continent(lat: f64, lon: f64) -> Option<&'static str> {
    // North America: 15°N to 60°N, -170° to -50°
    if (15.0..=60.0).contains(&lat) && (-170.0..=-50.0).contains(&lon) {
        return Some("North_America");
    }

    // South America: -60° to 15°N, -90° to -30°
    if (-60.0..=15.0).contains(&lat) && (-90.0..=-30.0).contains(&lon) {
        return Some("South_America");
    }

    // Australia: -50° to -10°, 110° to 180°
    if (-50.0..=-10.0).contains(&lat) && (110.0..=180.0).contains(&lon) {
        return Some("Australia");
    }

    // Africa: -35° to 35°N, -20° to 55°
    if (-35.0..=35.0).contains(&lat) && (-20.0..=55.0).contains(&lon) {
        return Some("Africa");
    }

    // Eurasia: 0° to 60°N, -15° to 180° (catch-all for remaining landmass)
    if (0.0..=60.0).contains(&lat) && (-15.0..=180.0).contains(&lon) {
        return Some("Eurasia");
    }

    // Islands, Antarctica, or ocean areas not covered
    None
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

/// Parse a coordinate value from filename components.
///
/// Converts prefix ("N", "S", "E", "W") and value string to a float.
/// "N" and "E" are positive, "S" and "W" are negative.
fn parse_coord_from_components(prefix: &str, value: &str) -> f64 {
    let val: f64 = value.parse().unwrap_or(0.0);
    match prefix {
        "S" | "W" => -val,
        _ => val,
    }
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

    #[test]
    fn test_coords_to_continent() {
        // North America
        assert_eq!(coords_to_continent(40.0, -100.0), Some("North_America"));
        assert_eq!(coords_to_continent(36.0, -117.0), Some("North_America")); // Death Valley

        // South America
        assert_eq!(coords_to_continent(-4.0, -61.0), Some("South_America")); // Amazon
        assert_eq!(coords_to_continent(-34.0, -58.0), Some("South_America")); // Buenos Aires

        // Australia
        assert_eq!(coords_to_continent(-34.0, 151.0), Some("Australia")); // Sydney
        assert_eq!(coords_to_continent(-25.0, 133.0), Some("Australia")); // Central Australia

        // Africa
        assert_eq!(coords_to_continent(30.0, 31.0), Some("Africa")); // Cairo
        assert_eq!(coords_to_continent(-34.0, 18.0), Some("Africa")); // Cape Town

        // Eurasia
        assert_eq!(coords_to_continent(35.0, 138.0), Some("Eurasia")); // Mount Fuji
        assert_eq!(coords_to_continent(51.0, 0.0), Some("Eurasia")); // London
        assert_eq!(coords_to_continent(55.0, 37.0), Some("Eurasia")); // Moscow
    }

    #[test]
    fn test_coords_to_continent_edge_cases() {
        // Boundaries
        assert_eq!(coords_to_continent(15.0, -170.0), Some("North_America")); // Edge of NA
        assert_eq!(coords_to_continent(60.0, -50.0), Some("North_America")); // NE corner

        // Areas outside defined continents
        assert_eq!(coords_to_continent(-70.0, 0.0), None); // Antarctica
        assert_eq!(coords_to_continent(0.0, -150.0), None); // Pacific Ocean
    }

    #[test]
    fn test_ardupilot_config() {
        // Default ardupilot() uses SRTM1
        let config = DownloadConfig::ardupilot();
        assert!(matches!(config.source, SrtmSource::ArduPilotSrtm1));
        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(config.max_retries, 3);

        // Explicit SRTM1
        let config = DownloadConfig::ardupilot_srtm1();
        assert!(matches!(config.source, SrtmSource::ArduPilotSrtm1));

        // Explicit SRTM3
        let config = DownloadConfig::ardupilot_srtm3();
        assert!(matches!(config.source, SrtmSource::ArduPilotSrtm3));
    }

    #[test]
    fn test_build_url_ardupilot_srtm1() {
        let config = DownloadConfig::ardupilot_srtm1();
        let downloader = Downloader::new(config).unwrap();

        // SRTM1 uses flat structure (no continent subdirectories)

        // Mount Fuji
        let url = downloader.build_url("N35E138").unwrap();
        assert_eq!(url, "https://terrain.ardupilot.org/SRTM1/N35E138.hgt.zip");

        // Death Valley
        let url = downloader.build_url("N36W117").unwrap();
        assert_eq!(url, "https://terrain.ardupilot.org/SRTM1/N36W117.hgt.zip");

        // Antarctica - works for SRTM1 (no continent check)
        let url = downloader.build_url("S70E000").unwrap();
        assert_eq!(url, "https://terrain.ardupilot.org/SRTM1/S70E000.hgt.zip");
    }

    #[test]
    fn test_build_url_ardupilot_srtm3() {
        let config = DownloadConfig::ardupilot_srtm3();
        let downloader = Downloader::new(config).unwrap();

        // Sydney (Australia)
        let url = downloader.build_url("S34E151").unwrap();
        assert_eq!(
            url,
            "https://terrain.ardupilot.org/SRTM3/Australia/S34E151.hgt.zip"
        );

        // Cape Town (Africa)
        let url = downloader.build_url("S34E018").unwrap();
        assert_eq!(
            url,
            "https://terrain.ardupilot.org/SRTM3/Africa/S34E018.hgt.zip"
        );

        // Amazon (South America)
        let url = downloader.build_url("S04W061").unwrap();
        assert_eq!(
            url,
            "https://terrain.ardupilot.org/SRTM3/South_America/S04W061.hgt.zip"
        );
    }

    #[test]
    fn test_build_url_ardupilot_srtm3_unknown_continent() {
        let config = DownloadConfig::ardupilot_srtm3();
        let downloader = Downloader::new(config).unwrap();

        // Antarctica - should fail for SRTM3 (requires continent mapping)
        let result = downloader.build_url("S70E000");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_url_custom_with_continent() {
        let config =
            DownloadConfig::with_url_template("https://example.com/{continent}/{filename}.hgt.zip");
        let downloader = Downloader::new(config).unwrap();

        let url = downloader.build_url("N35E138").unwrap();
        assert_eq!(url, "https://example.com/Eurasia/N35E138.hgt.zip");

        let url = downloader.build_url("N36W117").unwrap();
        assert_eq!(url, "https://example.com/North_America/N36W117.hgt.zip");
    }

    #[test]
    fn test_parse_coord_from_components() {
        assert_eq!(parse_coord_from_components("N", "35"), 35.0);
        assert_eq!(parse_coord_from_components("S", "35"), -35.0);
        assert_eq!(parse_coord_from_components("E", "138"), 138.0);
        assert_eq!(parse_coord_from_components("W", "117"), -117.0);
    }
}
