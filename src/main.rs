mod immich;
mod metadata;
mod tags;

use anyhow::{Context, Result};
use clap::Parser;
use immich::{
    has_empty_description, is_before_cutoff, is_favorite_asset, is_png_asset, ImmichClient,
};
use metadata::extract_parameters;
use std::thread;
use std::time::Duration;
use tags::extract_tags_from_info;

const ASSET_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug, Parser)]
#[command(name = "sd-immich-tagger")]
#[command(about = "Backfill Immich descriptions and tags from Stable Diffusion PNG metadata")]
struct Cli {
    /// Print planned updates without writing to Immich
    #[arg(long)]
    dry_run: bool,

    /// Maximum number of matching assets to process
    #[arg(long)]
    limit: Option<usize>,

    /// Immich server URL (falls back to IMMICH_URL)
    #[arg(long, env = "IMMICH_URL")]
    immich_url: Option<String>,

    /// Immich API key (falls back to IMMICH_API_KEY)
    #[arg(long, env = "IMMICH_API_KEY")]
    immich_api_key: Option<String>,
}

#[derive(Debug, Default)]
struct Summary {
    scanned: u32,
    candidates: u32,
    updated: u32,
    skipped_no_metadata: u32,
    failed: u32,
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    run(cli)
}

fn run(cli: Cli) -> Result<()> {
    let immich_url = cli
        .immich_url
        .filter(|value| !value.trim().is_empty())
        .context("IMMICH_URL is required (env var or --immich-url)")?;
    let immich_api_key = cli
        .immich_api_key
        .filter(|value| !value.trim().is_empty())
        .context("IMMICH_API_KEY is required (env var or --immich-api-key)")?;

    let client = ImmichClient::new(&immich_url, &immich_api_key)?;
    let mut summary = Summary::default();
    let mut page = 1u32;
    let mut processed = 0usize;

    if cli.dry_run {
        println!("Dry run mode: no changes will be written.");
    }

    loop {
        let search_page = client.search_image_assets(page)?;
        if search_page.items.is_empty() {
            break;
        }

        summary.scanned += search_page.count;

        for asset in search_page.items {
            if !is_png_asset(&asset)
                || !has_empty_description(&asset)
                || !is_favorite_asset(&asset)
                || !is_before_cutoff(&asset)
            {
                continue;
            }

            summary.candidates += 1;

            if let Some(limit) = cli.limit {
                if processed >= limit {
                    print_summary(&summary);
                    return Ok(());
                }
            }

            let file_name = asset
                .original_file_name
                .clone()
                .unwrap_or_else(|| asset.id.clone());

            match process_asset(&client, &asset.id, &file_name, cli.dry_run) {
                Ok(ProcessOutcome::Updated { tag_count }) => {
                    processed += 1;
                    summary.updated += 1;
                    println!(
                        "[ok] {file_name} ({}) -> description + {tag_count} tags",
                        asset.id
                    );
                }
                Ok(ProcessOutcome::SkippedNoMetadata) => {
                    processed += 1;
                    summary.skipped_no_metadata += 1;
                    println!(
                        "[skip] {file_name} ({}) -> no PNG parameters metadata",
                        asset.id
                    );
                }
                Err(error) => {
                    processed += 1;
                    summary.failed += 1;
                    eprintln!("[fail] {file_name} ({}) -> {error:#}", asset.id);
                }
            }

            thread::sleep(ASSET_DELAY);
        }

        if search_page.next_page.is_none() && search_page.count < immich::PAGE_SIZE {
            break;
        }
        page += 1;
    }

    print_summary(&summary);
    Ok(())
}

enum ProcessOutcome {
    Updated { tag_count: usize },
    SkippedNoMetadata,
}

fn process_asset(
    client: &ImmichClient,
    asset_id: &str,
    _file_name: &str,
    dry_run: bool,
) -> Result<ProcessOutcome> {
    let png_bytes = client
        .download_original(asset_id)
        .with_context(|| format!("failed to download asset {asset_id}"))?;

    let parameters = match extract_parameters(&png_bytes)? {
        Some(value) if !value.trim().is_empty() => value,
        _ => return Ok(ProcessOutcome::SkippedNoMetadata),
    };

    let tag_names = extract_tags_from_info(&parameters);

    if dry_run {
        println!(
            "[dry-run] {asset_id}: description length={}, tags={}",
            parameters.len(),
            tag_names.len()
        );
        return Ok(ProcessOutcome::Updated {
            tag_count: tag_names.len(),
        });
    }

    client
        .update_description(asset_id, &parameters)
        .with_context(|| format!("failed to update description for {asset_id}"))?;

    if !tag_names.is_empty() {
        let tag_ids = client
            .get_or_create_tag_ids(&tag_names)
            .with_context(|| format!("failed to resolve tags for {asset_id}"))?;
        client
            .link_tags(asset_id, &tag_ids)
            .with_context(|| format!("failed to link tags for {asset_id}"))?;
    }

    Ok(ProcessOutcome::Updated {
        tag_count: tag_names.len(),
    })
}

fn print_summary(summary: &Summary) {
    println!();
    println!("Summary");
    println!(
        "  matching search results (favorite, fileCreatedAt before {}, IMAGE): {}",
        immich::CUTOFF_DATE,
        summary.scanned
    );
    println!(
        "  candidates (favorite, before {}, empty description PNG): {}",
        immich::CUTOFF_DATE,
        summary.candidates
    );
    println!("  updated: {}", summary.updated);
    println!("  skipped (no metadata): {}", summary.skipped_no_metadata);
    println!("  failed: {}", summary.failed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_flags() {
        let cli = Cli::try_parse_from([
            "sd-immich-tagger",
            "--dry-run",
            "--limit",
            "10",
            "--immich-url",
            "http://example.com",
            "--immich-api-key",
            "secret",
        ])
        .unwrap();

        assert!(cli.dry_run);
        assert_eq!(cli.limit, Some(10));
        assert_eq!(cli.immich_url.as_deref(), Some("http://example.com"));
        assert_eq!(cli.immich_api_key.as_deref(), Some("secret"));
    }
}
