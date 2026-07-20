# sd-immich-tagger

<p align="center">
  <img src="images/icon.png" alt="sd-immich-tagger icon" width="160">
</p>

A Rust command-line tool that backfills Immich asset descriptions and tags from Stable Diffusion PNG metadata for assets uploaded before an uploader workflow was introduced.

## What it does

1. Search Immich for image assets that are:
   - favorited
   - have `fileCreatedAt` before a configurable cutoff datetime (via API `takenBefore`, not `createdBefore`)
2. Keep only PNG files with an empty description (checked via `exifInfo.description` from search with `withExif: true`)
3. Download the original PNG from Immich
4. Read the Stable Diffusion `parameters` text chunk
5. Update Immich:
   - `description` = full `parameters` text
   - tags = extracted from generation metadata (model, LoRA, prompt keywords, and related fields)

Existing tags are preserved; this tool only adds new tags.

## Download

Pre-built binaries for Linux, Windows, and macOS (Apple Silicon) are published on the [GitHub Releases](https://github.com/tenpaMk2/sd-immich-tagger/releases) page. Download the archive for your platform, extract it, and run `sd-immich-tagger` (or `sd-immich-tagger.exe` on Windows).

Version tags (for example `v1.0.0`) trigger a GitHub Actions workflow that builds and attaches these assets automatically.

## Setup

```bash
cp .env.example .env
# edit .env with your Immich URL, API key, and optional cutoff date
```

Required environment variables:

- `IMMICH_URL`
- `IMMICH_API_KEY`

Optional:

- `CUTOFF_DATE` — RFC3339 datetime; only assets with `fileCreatedAt` before this value are processed (default: `2026-06-20T00:00:00+09:00`)

You can also pass them as CLI flags: `--immich-url`, `--immich-api-key`, and `--cutoff-date`.

### API key permissions

Create an API key in Immich (Settings → API Keys) with at least these permissions:

| Permission       | Used for                    |
| ---------------- | --------------------------- |
| `asset.read`     | Search assets and list tags |
| `asset.download` | Download original PNG files |
| `asset.update`   | Write asset descriptions    |
| `tag.read`       | Look up existing tags       |
| `tag.create`     | Create missing tags         |
| `tag.asset`      | Link tags to assets         |

Dry-run mode still requires `asset.read` and `asset.download` because it downloads originals to read PNG metadata. If a request fails with `403 Forbidden` and a `Missing required permission` message, enable the permission listed in the error.

## Usage

Dry run first:

```bash
./sd-immich-tagger --dry-run
```

Apply updates:

```bash
./sd-immich-tagger
```

Limit processing while testing:

```bash
./sd-immich-tagger --dry-run --limit 50
```

Use a custom cutoff date:

```bash
./sd-immich-tagger --dry-run --cutoff-date 2026-01-01T00:00:00+09:00
```

### Building from source

This project is written in Rust. To build locally:

```bash
cargo build --release
./target/release/sd-immich-tagger --dry-run
```

## Output

The CLI prints per-asset progress and a final summary:

- matching search results (favorite, fileCreatedAt before cutoff, IMAGE)
- candidates (favorite, before cutoff, empty-description PNGs)
- updated
- skipped (PNG without `parameters` metadata)
- failed

## Notes

- Date filtering uses Immich API `takenBefore`, which maps to `fileCreatedAt`. It does not use `createdBefore` (Immich record `createdAt`).
- Only favorited PNG assets with `fileCreatedAt` before the cutoff date and an empty description are modified. Description emptiness is read from search results with `withExif: true`, so already-updated assets are skipped on subsequent runs.
- PNGs without Stable Diffusion metadata are skipped.
- A short delay is inserted between assets to reduce API pressure.

## License

MIT. See [LICENSE](LICENSE).
