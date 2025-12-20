# HTG Elevation Accuracy Benchmark Results

**Date:** 2025-12-20
**HTG Version:** 0.1.0
**SRTM Data:** SRTM3 (3 arc-second, ~90m resolution)

## Test Configuration

- **HTG Mode:** Bilinear interpolation (`?interpolate=true`)
- **Reference APIs:**
  - OpenTopoData (SRTM 90m dataset)
  - Open-Elevation

## Test Locations

| Location | Latitude | Longitude | Description |
|----------|----------|-----------|-------------|
| Mount Fuji | 35.3606 | 138.7274 | Japan's highest peak |
| Death Valley | 36.2308 | -116.7677 | Below sea level |
| Denver | 39.7392 | -104.9903 | Mile High City |
| Tokyo | 35.6762 | 139.6503 | Coastal city |
| Cape Town | -33.9249 | 18.4241 | Southern hemisphere |
| Amazon Basin | -3.1190 | -60.0217 | Tropical lowland |
| Swiss Alps | 46.5197 | 7.5597 | Steep terrain |
| La Paz | -16.5000 | -68.1500 | High altitude city |
| Grand Canyon | 36.0544 | -112.1401 | Dramatic terrain |
| Lhasa | 29.6500 | 91.1000 | Tibetan Plateau |

## Elevation Comparison Results

| Location | HTG (m) | OpenTopoData (m) | Open-Elevation (m) | Diff (OTD) | Diff (OE) |
|----------|---------|------------------|-------------------|------------|-----------|
| Mount Fuji | 3736.9 | 3737.0 | 3695.0 | -0.1 | +41.9 |
| Death Valley | -80.5 | -80.0 | -82.0 | -0.5 | +1.5 |
| Denver | 1601.4 | 1601.0 | 1603.0 | +0.4 | -1.6 |
| Tokyo | 39.4 | 39.0 | 41.0 | +0.4 | -1.6 |
| Cape Town | 13.9 | 14.0 | 14.0 | -0.1 | -0.1 |
| Amazon Basin | 44.8 | 45.0 | 47.0 | -0.2 | -2.2 |
| Swiss Alps | 1692.3 | 1692.0 | 1693.0 | +0.3 | -0.7 |
| La Paz | 3782.0 | 3782.0 | 3779.0 | +0.0 | +3.0 |
| Grand Canyon | 2101.6 | 2102.0 | 2096.0 | -0.4 | +5.6 |
| Lhasa | 3651.0 | 3651.0 | 3651.0 | -0.0 | -0.0 |

## Summary Statistics

### HTG vs OpenTopoData (SRTM 90m)

| Metric | Value |
|--------|-------|
| Mean Absolute Error | 0.2m |
| Max Error | 0.5m |
| Std Deviation | 0.2m |
| Within ±1m | 100% |
| Within ±5m | 100% |

### HTG vs Open-Elevation

| Metric | Value |
|--------|-------|
| Mean Absolute Error | 5.8m |
| Max Error | 41.9m |
| Std Deviation | 12.1m |
| Within ±1m | 30% |
| Within ±5m | 80% |

## Analysis

### OpenTopoData Comparison
HTG matches OpenTopoData almost perfectly (within 0.5m for all locations). This is expected since both use SRTM data. The small differences are due to:
- HTG uses bilinear interpolation while OpenTopoData may use different interpolation
- Slight floating-point precision differences

### Open-Elevation Discrepancy
The significant discrepancy at Mount Fuji (41.9m) suggests Open-Elevation may:
- Use a different elevation dataset
- Have data quality issues for certain regions
- Use different interpolation methods

## Conclusion

**HTG accuracy is validated.** The library correctly parses SRTM data and produces elevation values consistent with OpenTopoData's SRTM dataset within sub-meter accuracy.
