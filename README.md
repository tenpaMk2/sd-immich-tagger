# sd-immich-tagger

Backfill Immich asset descriptions and tags from Stable Diffusion PNG metadata for assets uploaded before the Immich uploader script was introduced.

## What it does

1. Search Immich for image assets that are:
   - favorited
   - have `fileCreatedAt` before `2026-06-20T00:00:00+09:00` (via API `takenBefore`, not `createdBefore`)
2. Keep only PNG files with an empty description (checked via `exifInfo.description` from search with `withExif: true`)
3. Download the original PNG from Immich
4. Read the Stable Diffusion `parameters` text chunk
5. Update Immich:
   - `description` = full `parameters` text
   - tags = extracted using the same rules as `immich_uploader.py`

Existing tags are preserved; this tool only adds new tags.

## Setup

```bash
cp .env.example .env
# edit .env with your Immich URL and API key
```

Required environment variables:

- `IMMICH_URL`
- `IMMICH_API_KEY`

You can also pass them as CLI flags: `--immich-url` and `--immich-api-key`.

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
cargo run -- --dry-run
```

Apply updates:

```bash
cargo run --
```

Limit processing while testing:

```bash
cargo run -- --dry-run --limit 50
```

Release build:

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
- Only favorited PNG assets with `fileCreatedAt` before `2026-06-20T00:00:00+09:00` and an empty description are modified. Description emptiness is read from search results with `withExif: true`, so already-updated assets are skipped on subsequent runs.
- PNGs without Stable Diffusion metadata are skipped.
- A short delay is inserted between assets to reduce API pressure.

## License

MIT. See [LICENSE](LICENSE).
