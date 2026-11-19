//! `diamond-stats ingest` and `diamond-stats serve`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use diamond_stats_api::serve;
use diamond_stats_core::Level;
use diamond_stats_ingest::{ClientConfig, HttpStatsClient, IngestOptions, ingest};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "diamond-stats",
    version,
    about = "Query MLB and Minor League statistics from a local Parquet cache"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Fetch one season from the MLB Stats API and write Parquet partitions.
    ///
    /// Game logs are not crawled. They are fetched the first time
    /// `GET /v1/players/{id}/gamelog` is called.
    Ingest {
        /// Season year, for example 2025.
        #[arg(long)]
        season: u16,
        /// Levels to ingest, comma-separated: mlb,aaa,aa,high_a,single_a,rookie.
        #[arg(long, value_delimiter = ',')]
        level: Vec<String>,
        /// Directory for the raw JSON cache and Parquet output.
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
    },
    /// Serve the HTTP API on loopback. Logs are JSON.
    Serve {
        /// Socket address. Defaults to loopback so the process is not a public bulk API.
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
        /// Directory written by `ingest`.
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest {
            season,
            level,
            data_dir,
        } => {
            init_tracing(false);
            if level.is_empty() {
                return Err("pass --level mlb,aaa (or another known level)".into());
            }
            let mut levels = Vec::with_capacity(level.len());
            for token in &level {
                levels.push(Level::parse(token).map_err(|err| err.to_string())?);
            }
            let client = HttpStatsClient::new(ClientConfig::new(&data_dir))?;
            let summary = ingest(
                &client,
                &IngestOptions {
                    data_dir: data_dir.clone(),
                    season,
                    levels,
                },
            )
            .await?;
            println!("ingested season {season} into {}", data_dir.display());
            for level_summary in &summary.levels {
                let hitting = &level_summary.groups[0];
                let pitching = &level_summary.groups[1];
                println!(
                    "  {level}: {} teams, {} hitting stints ({} multi-team players hydrated), {} pitching stints ({} multi-team players hydrated)",
                    level_summary.teams,
                    hitting.stints,
                    hitting.hydrated_players,
                    pitching.stints,
                    pitching.hydrated_players,
                    level = level_summary.level
                );
            }
            println!("game logs are fetched on the first /v1/players/{{id}}/gamelog request");
        }
        Command::Serve { bind, data_dir } => {
            init_tracing(true);
            println!("diamond-stats listening on http://{bind}");
            println!("swagger ui: http://{bind}/swagger-ui");
            serve(bind, data_dir).await?;
        }
    }
    Ok(())
}

fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.compact().init();
    }
}
