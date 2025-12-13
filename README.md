# icetable

A fast CLI for Apache Iceberg table management. Inspect, optimize, vacuum, and manage snapshots with support for REST catalogs (Polaris, Nessie, Tabular, etc.).

## Installation

### Linux / macOS

```bash
# Linux (x86_64)
curl -L https://github.com/laketoolkit/icetable/releases/latest/download/icetable-linux-x86_64 -o icetable
chmod +x icetable && sudo mv icetable /usr/local/bin/

# macOS (Apple Silicon)
curl -L https://github.com/laketoolkit/icetable/releases/latest/download/icetable-darwin-aarch64 -o icetable
chmod +x icetable && sudo mv icetable /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/laketoolkit/icetable/releases/latest/download/icetable-darwin-x86_64 -o icetable
chmod +x icetable && sudo mv icetable /usr/local/bin/
```

### Windows

Download `icetable-windows-x86_64.exe` from [Releases](https://github.com/laketoolkit/icetable/releases) and add to PATH.

### From source

```bash
cargo install --path .
# or
cargo build --release && sudo cp target/release/icetable /usr/local/bin/
```

## Quick Start

```bash
# Configure a catalog
icetable config add polaris https://polaris.example.com/api/catalog \
  --warehouse s3://warehouse

# Set default context (catalog + namespace + table)
icetable config use polaris -n analytics -t events

# Now all commands use this context
icetable inspect
icetable analyze
icetable snapshot list

# Or specify table explicitly with global flags
icetable inspect -n analytics -t events
```

## Global Flags

These flags work with any command:

| Flag | Description |
|------|-------------|
| `-t, --table <TABLE>` | Table name or path (e.g., `events` or `s3://bucket/path`) |
| `-n, --namespace <NAMESPACE>` | Namespace (e.g., `analytics` or `db.schema`) |
| `-q, --quiet` | Suppress non-error output |
| `--log-level <LEVEL>` | Log level (off, error, warn, info, debug, trace) |
| `--log-file <PATH>` | Write logs to file |

## Table Reference

Commands accept tables in three ways:

1. **Default context** - Set with `config use`, then omit flags:
   ```bash
   icetable config use polaris -n analytics -t orders
   icetable inspect  # uses configured context
   ```

2. **Global flags** `-n` and `-t`:
   ```bash
   icetable inspect -n analytics -t orders
   ```

3. **Direct path** with `-t`:
   ```bash
   icetable inspect -t s3://warehouse/db/orders
   ```

## REST Catalog Support

icetable integrates with Iceberg REST catalogs (Polaris, Nessie, Tabular, Unity Catalog).

### Configure a catalog

```bash
# Add catalog with OAuth2 authentication
icetable config add polaris https://polaris.example.com/api/catalog \
  --warehouse s3://warehouse \
  --client-id admin \
  --client-secret-env POLARIS_CLIENT_SECRET \
  --oauth2-endpoint https://polaris.example.com/api/catalog/v1/oauth/tokens \
  --oauth2-scope PRINCIPAL_ROLE:ALL

# Add catalog with bearer token
icetable config add polaris https://polaris.example.com/api/catalog \
  --warehouse s3://warehouse \
  --token-env POLARIS_TOKEN

# List configured catalogs and tables
icetable config ls
```

### Set working context

```bash
# Set catalog + namespace + table
icetable config use polaris -n analytics -t events

# Now commands use this context automatically
icetable inspect
icetable analyze
icetable optimize data --dry-run
```

### Catalog operations

```bash
# List namespaces
icetable ls -n ""

# List tables in namespace
icetable ls -n analytics

# Create namespace
icetable create -n analytics

# Create table (requires schema)
icetable create -n analytics -t events --schema schema.json

# Delete table
icetable delete -n analytics -t events --purge
```

## Commands

### inspect

Display table metadata, schema, partitioning, and current state.

```bash
icetable inspect
icetable inspect -n analytics -t orders
icetable inspect --output json
icetable inspect --verbose              # Show all properties and refs
icetable inspect --as-of 7d             # Time travel to 7 days ago
icetable inspect --snapshot 123456789   # Specific snapshot
```

### analyze

Analyze table health and get optimization recommendations.

```bash
icetable analyze
icetable analyze --verbose              # Show partition details
icetable analyze --skip-orphans         # Skip orphan file scan (faster)
icetable analyze --output json
```

### optimize

Compact small files into larger ones.

```bash
# Optimize data files
icetable optimize data --dry-run
icetable optimize data --all-partitions
icetable optimize data --partition "date=2024-01-*"
icetable optimize data --target-size 256mb
icetable optimize data --max-files 1000          # Incremental
icetable optimize data --max-bytes 10GB          # Limit by size

# Optimize manifests
icetable optimize manifests --dry-run
```

### vacuum

Remove orphan files not referenced by any snapshot.

```bash
icetable vacuum --dry-run
icetable vacuum
icetable vacuum --older-than 7d         # Only files older than 7 days
```

### snapshot

Manage table snapshots.

```bash
# List snapshots
icetable snapshot list
icetable snapshot list --all            # Show all (no limit)
icetable snapshot list --limit 20

# Show snapshot lineage (ancestor chain)
icetable snapshot lineage
icetable snapshot lineage --all

# Expire old snapshots
icetable snapshot expire --older-than 7d --dry-run
icetable snapshot expire --retain-last 10
icetable snapshot expire --ids 123,456,789

# Time travel - set current snapshot
icetable snapshot set --id 1234567890123
icetable snapshot set --as-of "2024-01-15T10:00:00Z"

# Create metadata backup
icetable snapshot create
```

### branch

Manage table branches (mutable named references).

```bash
icetable branch list
icetable branch create dev
icetable branch create feature --snapshot-id 123456789
icetable branch delete dev --dry-run
icetable branch rename dev development
```

### tag

Manage table tags (immutable named references).

```bash
icetable tag list
icetable tag create v1.0.0
icetable tag create v1.0.0 --snapshot-id 123456789
icetable tag delete v1.0.0 --dry-run
icetable tag rename v1.0.0 release-1.0.0
```

### repair

Fix table metadata issues.

```bash
# Remove references to missing files
icetable repair --remove-missing --dry-run

# Add orphan parquet files to table
icetable repair --add-orphans --dry-run

# Full sync
icetable repair --sync-metadata --dry-run
```

### config

Manage catalog configurations and working context.

```bash
# Set working context
icetable config use polaris -n analytics -t events

# Add catalog with authentication
icetable config add polaris https://polaris.example.com/api/catalog \
  --warehouse s3://warehouse \
  --client-id admin \
  --client-secret-env POLARIS_SECRET

# Add table alias (direct path)
icetable config add orders s3://warehouse/analytics/orders

# List all configurations
icetable config ls

# Delete configuration
icetable config delete polaris
```

### generate

Create synthetic Iceberg tables for testing and development. If the table already exists,
generates new data and appends it as a new snapshot.

```bash
# Generate table with predefined template
icetable generate -t s3://warehouse/test_table --template events --rows 10000 --files 5

# Available templates: events, transactions, sensors, users, web-logs
icetable generate -t /tmp/test --template sensors --rows 5000

# Custom schema
icetable generate -t s3://bucket/custom \
  --schema "id:long,name:string,amount:double,active:bool,ts:timestamp" \
  --rows 1000 --files 2

# Append to existing table
icetable generate -t s3://warehouse/events --template events --rows 1000 --force

# Reproducible with seed
icetable generate -t /tmp/test --template events --rows 1000 --seed 42

# Append to existing table (will prompt for confirmation)
icetable generate -t /tmp/test --template events --rows 5000

# Append without confirmation
icetable generate -t /tmp/test --template events --rows 5000 --force
```

### Other commands

```bash
# Initialize new empty table (local)
icetable init -t /tmp/new_table --schema schema.json

# Compare snapshots
icetable diff --from 123 --to 456

# View history
icetable history

# Compute statistics
icetable stats

# Validate table integrity
icetable validate

# Import from Delta Lake
icetable import delta -t s3://warehouse/iceberg --source s3://warehouse/delta

# Diagnose environment
icetable doctor
```

## Storage Support

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

## Resource Limits

Control memory and concurrency for large operations:

```bash
# Limit memory usage (useful for constrained environments)
icetable --max-memory 2GB optimize data --all-partitions

# Set operation timeout
icetable --timeout 3600 vacuum

# Limit concurrency
icetable --max-concurrency 4 optimize data --all-partitions
```

## Output Formats

Most commands support JSON output for scripting:

```bash
icetable inspect --output json
icetable analyze --output json
icetable snapshot list --output json | jq '.[].snapshot_id'
```

## Shell Completions

```bash
# Bash
icetable completions bash > ~/.bash_completion.d/icetable

# Zsh
icetable completions zsh > ~/.zfunc/_icetable

# Fish
icetable completions fish > ~/.config/fish/completions/icetable.fish
```

## Examples

### Daily maintenance workflow

```bash
# Set context once
icetable config use polaris -n analytics -t events

# Check health
icetable analyze

# Compact if needed
icetable optimize data --dry-run
icetable optimize data --all-partitions

# Expire old snapshots
icetable snapshot expire --older-than 7d --retain-last 5

# Clean orphans
icetable vacuum
```

### Debug table issue

```bash
icetable inspect --verbose
icetable snapshot list --all
icetable snapshot lineage

# Roll back to known good state
icetable snapshot set --id 1234567890123
```

### CI/CD pipeline

```bash
#!/bin/bash
set -e

# Validate table health
icetable analyze --output json | jq -e '.health_score > 80'

# Run incremental compaction
icetable optimize data --max-bytes 10GB --all-partitions

# Expire snapshots older than 30 days, keep at least 10
icetable snapshot expire --older-than 30d --retain-last 10

# Clean up orphans
icetable vacuum --older-than 7d
```

## License

Apache-2.0
