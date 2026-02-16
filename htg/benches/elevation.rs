use std::io::Write;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

const SRTM3_SAMPLES: usize = 1201;
const SRTM3_SIZE: usize = SRTM3_SAMPLES * SRTM3_SAMPLES * 2;

/// Create a synthetic SRTM3 tile with a simple elevation gradient.
fn create_tile(dir: &std::path::Path, filename: &str) {
    let mut data = vec![0u8; SRTM3_SIZE];
    for row in 0..SRTM3_SAMPLES {
        for col in 0..SRTM3_SAMPLES {
            let elev = ((row + col) % 4000) as i16;
            let offset = (row * SRTM3_SAMPLES + col) * 2;
            let bytes = elev.to_be_bytes();
            data[offset] = bytes[0];
            data[offset + 1] = bytes[1];
        }
    }
    let path = dir.join(filename);
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&data).unwrap();
}

fn bench_single_nearest(c: &mut Criterion) {
    let tmp = TempDir::new().unwrap();
    create_tile(tmp.path(), "N35E138.hgt");
    let service = htg::SrtmService::new(tmp.path(), 10);

    // Warm the cache
    let _ = service.get_elevation(35.5, 138.5);

    c.bench_function("single_nearest_cached", |b| {
        b.iter(|| {
            black_box(
                service
                    .get_elevation(black_box(35.3606), black_box(138.7274))
                    .unwrap(),
            );
        });
    });
}

fn bench_single_interpolated(c: &mut Criterion) {
    let tmp = TempDir::new().unwrap();
    create_tile(tmp.path(), "N35E138.hgt");
    let service = htg::SrtmService::new(tmp.path(), 10);

    // Warm the cache
    let _ = service.get_elevation(35.5, 138.5);

    c.bench_function("single_interpolated_cached", |b| {
        b.iter(|| {
            black_box(
                service
                    .get_elevation_interpolated(black_box(35.3606), black_box(138.7274))
                    .unwrap(),
            );
        });
    });
}

fn bench_batch_same_tile(c: &mut Criterion) {
    let tmp = TempDir::new().unwrap();
    create_tile(tmp.path(), "N35E138.hgt");
    let service = htg::SrtmService::new(tmp.path(), 10);

    // Generate 1000 coords within the same tile
    let coords: Vec<(f64, f64)> = (0..1000)
        .map(|i| {
            let frac = i as f64 / 1000.0;
            (35.0 + frac * 0.99, 138.0 + frac * 0.99)
        })
        .collect();

    // Warm the cache
    let _ = service.get_elevation(35.5, 138.5);

    c.bench_function("batch_1000_same_tile", |b| {
        b.iter(|| {
            black_box(service.get_elevations_batch(black_box(&coords), 0));
        });
    });
}

fn bench_batch_multi_tile(c: &mut Criterion) {
    let tmp = TempDir::new().unwrap();
    create_tile(tmp.path(), "N35E138.hgt");
    create_tile(tmp.path(), "N36E138.hgt");
    create_tile(tmp.path(), "N35E139.hgt");
    let service = htg::SrtmService::new(tmp.path(), 10);

    // Generate 1000 coords spread across 3 tiles
    let coords: Vec<(f64, f64)> = (0..1000)
        .map(|i| match i % 3 {
            0 => (35.0 + (i as f64 / 3000.0) * 0.99, 138.5),
            1 => (36.0 + (i as f64 / 3000.0) * 0.99, 138.5),
            _ => (35.0 + (i as f64 / 3000.0) * 0.99, 139.5),
        })
        .collect();

    // Warm the cache
    let _ = service.get_elevation(35.5, 138.5);
    let _ = service.get_elevation(36.5, 138.5);
    let _ = service.get_elevation(35.5, 139.5);

    c.bench_function("batch_1000_multi_tile", |b| {
        b.iter(|| {
            black_box(service.get_elevations_batch(black_box(&coords), 0));
        });
    });
}

criterion_group!(
    benches,
    bench_single_nearest,
    bench_single_interpolated,
    bench_batch_same_tile,
    bench_batch_multi_tile,
);
criterion_main!(benches);
