use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

/// SRTM elevation data CLI tool
#[derive(Parser)]
#[command(name = "htg")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Directory containing .hgt files
    #[arg(short, long, env = "HTG_DATA_DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Maximum tiles in cache
    #[arg(
        short,
        long,
        env = "HTG_CACHE_SIZE",
        default_value = "100",
        global = true
    )]
    cache_size: u64,

    /// Enable automatic tile download
    #[arg(short, long, global = true)]
    auto_download: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Query elevation for a single coordinate
    Query {
        /// Latitude in decimal degrees
        #[arg(long)]
        lat: f64,

        /// Longitude in decimal degrees
        #[arg(long)]
        lon: f64,

        /// Use bilinear interpolation for sub-pixel accuracy
        #[arg(short, long)]
        interpolate: bool,

        /// Output result as JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Process elevation for multiple coordinates from a file
    Batch {
        /// Input file (CSV or GeoJSON)
        input: PathBuf,

        /// Output file (same format as input if not specified)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Column name for latitude (CSV only)
        #[arg(long, default_value = "lat")]
        lat_col: String,

        /// Column name for longitude (CSV only)
        #[arg(long, default_value = "lon")]
        lon_col: String,

        /// Use bilinear interpolation
        #[arg(short, long)]
        interpolate: bool,
    },

    /// Display information about an SRTM tile
    Info {
        /// Path to .hgt file, or tile name (e.g., N35E138)
        tile: String,

        /// Specify tile by latitude instead of filename
        #[arg(long, conflicts_with = "tile")]
        lat: Option<f64>,

        /// Specify tile by longitude instead of filename
        #[arg(long, conflicts_with = "tile")]
        lon: Option<f64>,
    },

    /// List available SRTM tiles
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Query {
            lat,
            lon,
            interpolate,
            json,
        } => commands::query::run(
            cli.data_dir,
            cli.cache_size,
            cli.auto_download,
            lat,
            lon,
            interpolate,
            json,
        ),
        Commands::Batch {
            input,
            output,
            lat_col,
            lon_col,
            interpolate,
        } => commands::batch::run(
            cli.data_dir,
            cli.cache_size,
            cli.auto_download,
            input,
            output,
            lat_col,
            lon_col,
            interpolate,
        ),
        Commands::Info { tile, lat, lon } => commands::info::run(cli.data_dir, tile, lat, lon),
        Commands::List => commands::list::run(cli.data_dir),
    }
}
