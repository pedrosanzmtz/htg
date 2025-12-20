//! SRTM filename utilities.
//!
//! This module provides functions for converting between coordinates and
//! SRTM `.hgt` filenames.
//!
//! # Filename Format
//!
//! SRTM files follow the naming convention: `{N|S}{lat}{E|W}{lon}.hgt`
//!
//! - Latitude: 2 digits with N/S prefix (e.g., N35, S12)
//! - Longitude: 3 digits with E/W prefix (e.g., E138, W077)
//!
//! The filename represents the **southwest corner** of the 1° × 1° tile.

/// Convert latitude and longitude to an SRTM `.hgt` filename.
///
/// # Arguments
///
/// * `lat` - Latitude in decimal degrees (-60 to 60)
/// * `lon` - Longitude in decimal degrees (-180 to 180)
///
/// # Returns
///
/// The filename (e.g., "N35E138.hgt")
///
/// # Examples
///
/// ```
/// use htg::filename::lat_lon_to_filename;
///
/// assert_eq!(lat_lon_to_filename(35.5, 138.7), "N35E138.hgt");
/// assert_eq!(lat_lon_to_filename(-12.3, -77.1), "S13W078.hgt");
/// assert_eq!(lat_lon_to_filename(0.5, -0.5), "N00W001.hgt");
/// ```
pub fn lat_lon_to_filename(lat: f64, lon: f64) -> String {
    let lat_int = lat.floor() as i32;
    let lon_int = lon.floor() as i32;

    let lat_prefix = if lat_int >= 0 { 'N' } else { 'S' };
    let lon_prefix = if lon_int >= 0 { 'E' } else { 'W' };

    format!(
        "{}{:02}{}{:03}.hgt",
        lat_prefix,
        lat_int.abs(),
        lon_prefix,
        lon_int.abs()
    )
}

/// Parse an SRTM filename to extract the base coordinates.
///
/// # Arguments
///
/// * `filename` - The filename (with or without path, with or without extension)
///
/// # Returns
///
/// The (latitude, longitude) of the southwest corner, or `None` if parsing fails.
///
/// # Examples
///
/// ```
/// use htg::filename::filename_to_lat_lon;
///
/// assert_eq!(filename_to_lat_lon("N35E138.hgt"), Some((35, 138)));
/// assert_eq!(filename_to_lat_lon("S12W077.hgt"), Some((-12, -77)));
/// assert_eq!(filename_to_lat_lon("/path/to/N00E000.hgt"), Some((0, 0)));
/// assert_eq!(filename_to_lat_lon("invalid"), None);
/// ```
pub fn filename_to_lat_lon(filename: &str) -> Option<(i32, i32)> {
    // Extract just the filename if a path is given
    let name = filename
        .rsplit('/')
        .next()
        .unwrap_or(filename)
        .rsplit('\\')
        .next()
        .unwrap_or(filename);

    // Remove .hgt extension if present
    let name = name.strip_suffix(".hgt").unwrap_or(name);

    // Must be exactly 7 characters: N00E000
    if name.len() != 7 {
        return None;
    }

    let chars: Vec<char> = name.chars().collect();

    // Parse latitude
    let lat_sign = match chars[0] {
        'N' | 'n' => 1,
        'S' | 's' => -1,
        _ => return None,
    };
    let lat: i32 = name[1..3].parse().ok()?;

    // Parse longitude
    let lon_sign = match chars[3] {
        'E' | 'e' => 1,
        'W' | 'w' => -1,
        _ => return None,
    };
    let lon: i32 = name[4..7].parse().ok()?;

    Some((lat * lat_sign, lon * lon_sign))
}

/// Validate that coordinates are within SRTM coverage.
///
/// SRTM data covers latitudes from -60° to +60° and all longitudes.
///
/// # Arguments
///
/// * `lat` - Latitude in decimal degrees
/// * `lon` - Longitude in decimal degrees
///
/// # Returns
///
/// `true` if the coordinates are within SRTM coverage.
pub fn is_valid_srtm_coord(lat: f64, lon: f64) -> bool {
    (-60.0..=60.0).contains(&lat) && (-180.0..=180.0).contains(&lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positive_coords() {
        assert_eq!(lat_lon_to_filename(35.5, 138.7), "N35E138.hgt");
        assert_eq!(lat_lon_to_filename(0.5, 0.5), "N00E000.hgt");
        assert_eq!(lat_lon_to_filename(1.0, 1.0), "N01E001.hgt");
        assert_eq!(lat_lon_to_filename(59.9, 179.9), "N59E179.hgt");
    }

    #[test]
    fn test_negative_coords() {
        // floor(-12.3) = -13, floor(-77.1) = -78
        assert_eq!(lat_lon_to_filename(-12.3, -77.1), "S13W078.hgt");
        // floor(-0.5) = -1
        assert_eq!(lat_lon_to_filename(-0.5, -0.5), "S01W001.hgt");
        assert_eq!(lat_lon_to_filename(-1.0, -1.0), "S01W001.hgt");
        // floor(-59.9) = -60, floor(-179.9) = -180
        assert_eq!(lat_lon_to_filename(-59.9, -179.9), "S60W180.hgt");
    }

    #[test]
    fn test_mixed_coords() {
        // floor(-122.4) = -123
        assert_eq!(lat_lon_to_filename(35.5, -122.4), "N35W123.hgt"); // San Francisco area
                                                                      // floor(-33.9) = -34
        assert_eq!(lat_lon_to_filename(-33.9, 151.2), "S34E151.hgt"); // Sydney area
                                                                      // floor(-99.1) = -100
        assert_eq!(lat_lon_to_filename(19.4, -99.1), "N19W100.hgt"); // Mexico City area
    }

    #[test]
    fn test_boundary_cases() {
        // Exactly on tile boundary
        assert_eq!(lat_lon_to_filename(35.0, 138.0), "N35E138.hgt");
        assert_eq!(lat_lon_to_filename(-35.0, -138.0), "S35W138.hgt");

        // Equator and prime meridian
        assert_eq!(lat_lon_to_filename(0.0, 0.0), "N00E000.hgt");
        assert_eq!(lat_lon_to_filename(0.1, 0.1), "N00E000.hgt");
        // floor(-0.1) = -1
        assert_eq!(lat_lon_to_filename(-0.1, -0.1), "S01W001.hgt");
    }

    #[test]
    fn test_parse_filename() {
        assert_eq!(filename_to_lat_lon("N35E138.hgt"), Some((35, 138)));
        assert_eq!(filename_to_lat_lon("S12W077.hgt"), Some((-12, -77)));
        assert_eq!(filename_to_lat_lon("N00E000.hgt"), Some((0, 0)));
        assert_eq!(filename_to_lat_lon("S00W000.hgt"), Some((0, 0)));
    }

    #[test]
    fn test_parse_filename_with_path() {
        assert_eq!(
            filename_to_lat_lon("/path/to/data/N35E138.hgt"),
            Some((35, 138))
        );
        assert_eq!(
            filename_to_lat_lon("C:\\data\\S12W077.hgt"),
            Some((-12, -77))
        );
    }

    #[test]
    fn test_parse_filename_invalid() {
        assert_eq!(filename_to_lat_lon("invalid"), None);
        assert_eq!(filename_to_lat_lon("N35E13.hgt"), None); // Too short
        assert_eq!(filename_to_lat_lon("X35E138.hgt"), None); // Invalid prefix
        assert_eq!(filename_to_lat_lon("N35X138.hgt"), None); // Invalid prefix
        assert_eq!(filename_to_lat_lon("NAAE138.hgt"), None); // Non-numeric
    }

    #[test]
    fn test_parse_case_insensitive() {
        assert_eq!(filename_to_lat_lon("n35e138.hgt"), Some((35, 138)));
        assert_eq!(filename_to_lat_lon("s12w077.hgt"), Some((-12, -77)));
    }

    #[test]
    fn test_roundtrip() {
        let test_coords = [
            (35.5, 138.7),
            (-12.3, -77.1),
            (0.5, -0.5),
            (-0.5, 0.5),
            (59.9, 179.9),
            (-59.9, -179.9),
        ];

        for (lat, lon) in test_coords {
            let filename = lat_lon_to_filename(lat, lon);
            let (parsed_lat, parsed_lon) = filename_to_lat_lon(&filename).unwrap();

            assert_eq!(parsed_lat, lat.floor() as i32);
            assert_eq!(parsed_lon, lon.floor() as i32);
        }
    }

    #[test]
    fn test_is_valid_srtm_coord() {
        // Valid coordinates
        assert!(is_valid_srtm_coord(0.0, 0.0));
        assert!(is_valid_srtm_coord(60.0, 180.0));
        assert!(is_valid_srtm_coord(-60.0, -180.0));
        assert!(is_valid_srtm_coord(35.5, 138.7));

        // Invalid coordinates
        assert!(!is_valid_srtm_coord(61.0, 0.0)); // Lat too high
        assert!(!is_valid_srtm_coord(-61.0, 0.0)); // Lat too low
        assert!(!is_valid_srtm_coord(0.0, 181.0)); // Lon too high
        assert!(!is_valid_srtm_coord(0.0, -181.0)); // Lon too low
    }
}
