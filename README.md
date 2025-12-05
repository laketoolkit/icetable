# icetable

A fast, modern CLI for Apache Iceberg table management.

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary at ./target/release/icetable
```

## Quick Start

```bash
# Inspect a table
icetable inspect s3://warehouse/my_table

# Analyze table health
icetable analyze s3://warehouse/my_table

# List snapshots
icetable snapshot list s3://warehouse/my_table

# Optimize small files
icetable optimize data s3://warehouse/my_table --dry-run

# Clean up old snapshots
icetable snapshot expire s3://warehouse/my_table --older-than 7d --dry-run

# Remove orphan files
icetable vacuum s3://warehouse/my_table --dry-run
```

## Commands

### inspect

Display table metadata including schema, partitioning, snapshots, and statistics.

```bash
icetable inspect <TABLE_PATH>
icetable inspect s3://warehouse/orders --output json
```

### analyze

Analyze table health and get optimization recommendations.

```bash
icetable analyze <TABLE_PATH>
icetable analyze s3://warehouse/orders --verbose
icetable analyze s3://warehouse/orders --skip-orphans  # Skip orphan file scan
```

Output shows a summary table with status indicators:
- `✓` (green) - OK
- `⚠` (yellow) - Needs attention
- `✗` (red) - Critical issue

### optimize

Optimize table performance by compacting files.

```bash
# Compact small data files
icetable optimize data <TABLE_PATH> --dry-run
icetable optimize data s3://warehouse/orders --partition "date=2024-01-01"

# Rewrite manifests
icetable optimize manifests <TABLE_PATH> --dry-run
```

### vacuum

Remove orphan files (data files not referenced by any snapshot).

```bash
icetable vacuum <TABLE_PATH> --dry-run
icetable vacuum s3://warehouse/orders --older-than 7d
```

### snapshot

Manage table snapshots.

```bash
# List all snapshots
icetable snapshot list <TABLE_PATH>

# Show snapshot lineage (ancestor chain)
icetable snapshot lineage <TABLE_PATH>
icetable snapshot lineage <TABLE_PATH> --all           # Show full history
icetable snapshot lineage <TABLE_PATH> -n 20           # Show last 20

# Expire old snapshots
icetable snapshot expire <TABLE_PATH> --older-than 7d --dry-run
icetable snapshot expire <TABLE_PATH> --older-than 30d

# Time travel - set current snapshot
icetable snapshot set <TABLE_PATH> --id <SNAPSHOT_ID>
icetable snapshot set <TABLE_PATH> --as-of "2024-01-15T10:00:00Z"

# Cherry-pick changes from a snapshot
icetable snapshot cherrypick <TABLE_PATH> <SNAPSHOT_ID>
```

### branch

Manage table branches (mutable named references).

```bash
# List branches
icetable branch list <TABLE_PATH>

# Create a branch
icetable branch create <TABLE_PATH> <BRANCH_NAME>
icetable branch create <TABLE_PATH> dev --snapshot-id 123456789

# Delete a branch
icetable branch delete <TABLE_PATH> <BRANCH_NAME> --dry-run

# Rename a branch
icetable branch rename <TABLE_PATH> <OLD_NAME> <NEW_NAME>
```

### tag

Manage table tags (immutable named references).

```bash
# List tags
icetable tag list <TABLE_PATH>

# Create a tag
icetable tag create <TABLE_PATH> <TAG_NAME>
icetable tag create <TABLE_PATH> v1.0.0 --snapshot-id 123456789

# Delete a tag
icetable tag delete <TABLE_PATH> <TAG_NAME> --dry-run

# Rename a tag
icetable tag rename <TABLE_PATH> <OLD_NAME> <NEW_NAME>
```

### repair

Fix table metadata issues by syncing with actual storage state.

```bash
# Remove references to files that no longer exist on storage
icetable repair <TABLE_PATH> --remove-missing --dry-run

# Add orphan parquet files (on storage but not in metadata) to the table
icetable repair <TABLE_PATH> --add-orphans --dry-run

# Full sync: remove missing references AND add orphan files
icetable repair <TABLE_PATH> --sync-metadata --dry-run
```

## Storage Support

icetable supports multiple storage backends:

- **Local filesystem**: `/path/to/table`
- **Amazon S3**: `s3://bucket/path/to/table`
- **MinIO**: `s3://bucket/path` (with `AWS_ENDPOINT_URL`)
- **Google Cloud Storage**: `gs://bucket/path/to/table`
- **Azure Blob Storage**: `az://container/path/to/table`

### S3/MinIO Configuration

```bash
export AWS_ACCESS_KEY_ID=your_key
export AWS_SECRET_ACCESS_KEY=your_secret
export AWS_REGION=us-east-1

# For MinIO or S3-compatible storage
export AWS_ENDPOINT_URL=http://localhost:9000
```

## Output Formats

All commands support JSON output for scripting:

```bash
icetable inspect s3://warehouse/orders --output json
icetable snapshot list s3://warehouse/orders --output json | jq '.[] | .id'
```

## Common Options

| Option | Description |
|--------|-------------|
| `--output json` | Output as JSON instead of formatted tables |
| `--dry-run` | Preview changes without applying them |
| `--verbose` | Show detailed information |
| `--help` | Show help for command |

## Examples

### Daily maintenance workflow

```bash
# 1. Check table health
icetable analyze s3://warehouse/orders

# 2. Compact small files if needed
icetable optimize data s3://warehouse/orders --dry-run
icetable optimize data s3://warehouse/orders

# 3. Expire old snapshots (keep 7 days)
icetable snapshot expire s3://warehouse/orders --older-than 7d --dry-run
icetable snapshot expire s3://warehouse/orders --older-than 7d

# 4. Remove orphan files
icetable vacuum s3://warehouse/orders --dry-run
icetable vacuum s3://warehouse/orders
```

### Debug a table issue

```bash
# View current state
icetable inspect s3://warehouse/orders

# Check snapshot history
icetable snapshot list s3://warehouse/orders

# View lineage of current snapshot
icetable snapshot lineage s3://warehouse/orders

# Roll back to previous snapshot
icetable snapshot set s3://warehouse/orders --id 1234567890123
```

### Tag a release

```bash
# Create a tag for the current snapshot
icetable tag create s3://warehouse/orders v2.0.0

# Or tag a specific snapshot
icetable tag create s3://warehouse/orders v1.5.0 --snapshot-id 1234567890123
```

## Shell Completions

Generate shell completions for your preferred shell:

```bash
# Bash
icetable completions bash > ~/.bash_completion.d/icetable

# Zsh
icetable completions zsh > ~/.zfunc/_icetable

# Fish
icetable completions fish > ~/.config/fish/completions/icetable.fish

# PowerShell
icetable completions powershell > icetable.ps1
```

## License

Apache-2.0
