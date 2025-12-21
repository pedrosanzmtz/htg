# htg-service

[![Docker Hub](https://img.shields.io/docker/v/pedrosanzmtz/htg-service?label=version)](https://hub.docker.com/r/pedrosanzmtz/htg-service)
[![Docker Pulls](https://img.shields.io/docker/pulls/pedrosanzmtz/htg-service)](https://hub.docker.com/r/pedrosanzmtz/htg-service)

High-performance HTTP microservice for querying SRTM elevation data.

## Quick Start

```bash
docker run -d \
  -p 8080:8080 \
  -v /path/to/hgt/files:/data/srtm:ro \
  -e HTG_DATA_DIR=/data/srtm \
  pedrosanzmtz/htg-service:latest
```

Test it:
```bash
curl "http://localhost:8080/elevation?lat=35.6762&lon=139.6503"
# {"elevation":40,"lat":35.6762,"lon":139.6503}
```

## Auto-Download Mode

No local `.hgt` files? Enable auto-download from ArduPilot terrain server:

```bash
docker run -d \
  -p 8080:8080 \
  -v htg-cache:/data/srtm \
  -e HTG_DATA_DIR=/data/srtm \
  -e HTG_DOWNLOAD_SOURCE=ardupilot \
  pedrosanzmtz/htg-service:latest
```

Tiles are downloaded automatically on first request.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `HTG_DATA_DIR` | `/data/srtm` | Directory containing `.hgt` files |
| `HTG_CACHE_SIZE` | `100` | Maximum tiles in memory |
| `HTG_PORT` | `8080` | HTTP server port |
| `HTG_DOWNLOAD_SOURCE` | - | Auto-download: `ardupilot`, `ardupilot-srtm1`, `ardupilot-srtm3` |
| `HTG_DOWNLOAD_URL` | - | Custom URL template (e.g., `https://example.com/{filename}.hgt.gz`) |
| `RUST_LOG` | `info` | Log level: `debug`, `info`, `warn`, `error` |

## Docker Compose

```yaml
services:
  htg:
    image: pedrosanzmtz/htg-service:latest
    ports:
      - "8080:8080"
    volumes:
      - ./data/srtm:/data/srtm:ro
    environment:
      - HTG_DATA_DIR=/data/srtm
      - HTG_CACHE_SIZE=100
      - HTG_DOWNLOAD_SOURCE=ardupilot
    restart: unless-stopped
```

## API Endpoints

### GET /elevation

Query elevation for a coordinate.

```bash
# Basic query
curl "http://localhost:8080/elevation?lat=35.3606&lon=138.7274"
# {"elevation":3776,"lat":35.3606,"lon":138.7274}

# With bilinear interpolation
curl "http://localhost:8080/elevation?lat=35.3606&lon=138.7274&interpolate=true"
# {"elevation":3776.42,"lat":35.3606,"lon":138.7274,"interpolated":true}
```

### POST /elevation

Batch query with GeoJSON geometry.

```bash
curl -X POST "http://localhost:8080/elevation" \
  -H "Content-Type: application/json" \
  -d '{"type":"LineString","coordinates":[[138.5,35.5],[139.0,35.0]]}'
```

### GET /health

Health check endpoint.

```bash
curl "http://localhost:8080/health"
# {"status":"healthy","version":"0.1.0"}
```

### GET /stats

Cache statistics.

```bash
curl "http://localhost:8080/stats"
# {"cached_tiles":5,"cache_hits":150,"cache_misses":5,"hit_rate":0.967}
```

### GET /docs

Interactive OpenAPI documentation (Swagger UI).

## Performance

| Metric | Value |
|--------|-------|
| Memory (100 SRTM3 tiles) | ~280MB |
| Memory (100 SRTM1 tiles) | ~2.5GB |
| Cached response | <10ms |
| Throughput | >10,000 req/s |

## SRTM Data

Download `.hgt` files from:
- [SRTM Tile Grabber](https://dwtkns.com/srtm30m/) - Interactive map
- [USGS Earth Explorer](https://earthexplorer.usgs.gov/) - Official source

Or use `HTG_DOWNLOAD_SOURCE=ardupilot` for automatic downloads.

## Source Code

[github.com/pedrosanzmtz/htg](https://github.com/pedrosanzmtz/htg)

## License

MIT
