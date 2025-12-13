# icetable

A fast CLI for Apache Iceberg table management. Inspect, optimize, vacuum, and manage snapshots with support for REST catalogs (Nessie, Polaris, etc.).

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
# Set a default table context (optional, avoids repeating -t)
icetable config use my_table s3://warehouse/db/my_table

# Inspect the table
icetable inspect

# Or specify table explicitly
icetable inspect -t s3://warehouse/db/my_table

# Analyze table health
icetable analyze

# List snapshots
icetable snapshot list

# Optimize small files (dry-run first)
icetable optimize --dry-run
icetable optimize

# Expire old snapshots
icetable snapshot expire --older-than 7d --dry-run

# Remove orphan files
icetable vacuum --dry-run
```

## Table Reference

Commands accept tables in three ways:

1. **Default context** - Set once with `config use`, then omit `-t`:
   ```bash
   icetable config use orders s3://warehouse/orders
   icetable inspect  # uses configured default
   ```

2. **Explicit path** with `-t/--table`:
   ```bash
   icetable inspect -t s3://warehouse/orders
   ```

3. **Catalog reference** with `--catalog-uri`:
   ```bash
   icetable --catalog-uri http://nessie:19120/api/v2 inspect -t analytics.orders
   ```

## REST Catalog Support

icetable integrates with Iceberg REST catalogs (Nessie, Polaris, Tabular, Unity Catalog).

### Configure a catalog

```bash
# Add catalog to config
icetable config add-catalog nessie http://nessie:19120/api/v2

# List namespaces
icetable catalog -c nessie namespaces

# List tables in a namespace
icetable catalog -c nessie -n analytics tables

# Inspect a catalog table
icetable config use my_table nessie.analytics.orders
icetable inspect
```

### Or use CLI flags directly

```bash
# Set env vars
export ICETABLE_CATALOG_URI=http://nessie:19120/api/v2

# Or use flags
icetable --catalog-uri http://nessie:19120/api/v2 inspect -t analytics.orders
```

### Catalog operations

```bash
# Create namespace
icetable catalog -c nessie create-namespace analytics

# Create table (requires schema file)
icetable catalog -c nessie create-table -n analytics --name events --schema schema.json

# Drop table
icetable catalog -c nessie drop-table -n analytics --name events --purge
```

When using a catalog, write operations (`snapshot expire`, `snapshot set`) commit changes through the catalog's REST API for atomic commits with conflict detection.

## Commands

### inspect

Display table metadata, schema, partitioning, and data preview.

```bash
icetable inspect
icetable inspect -t s3://warehouse/orders --output json
icetable inspect --schema          # Only show schema
icetable inspect --metadata        # Show metadata details
icetable inspect --preview         # Show data preview
icetable inspect --stats           # Show column statistics
icetable inspect --as-of 7d        # Time travel to 7 days ago
```

### analyze

Analyze table health and get optimization recommendations.

```bash
icetable analyze
icetable analyze --verbose
icetable analyze --skip-orphans    # Skip orphan file scan (faster)
```

### optimize

Compact small files into larger ones.

```bash
icetable optimize --dry-run
icetable optimize
icetable optimize --partition "date=2024-01-*"   # Only specific partitions
icetable optimize --target-size 256mb            # Target file size
icetable optimize --max-files 1000               # Incremental compaction
```

### vacuum

Remove orphan files not referenced by any snapshot.

```bash
icetable vacuum --dry-run
icetable vacuum
icetable vacuum --older-than 7d    # Only files older than 7 days
```

### snapshot

Manage table snapshots.

```bash
# List snapshots
icetable snapshot list
icetable snapshot list --all       # Show all (no limit)
icetable snapshot list -n 20       # Limit to 20

# Show snapshot lineage (ancestor chain)
icetable snapshot lineage
icetable snapshot lineage --all

# Expire old snapshots
icetable snapshot expire --older-than 7d --dry-run
icetable snapshot expire --retain-last 10         # Keep at least 10
icetable snapshot expire --ids 123,456,789        # Expire specific IDs

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

Manage table context and catalog configurations.

```bash
# Set default table
icetable config use orders s3://warehouse/orders
icetable config current
icetable config unset

# Manage table aliases
icetable config add orders s3://warehouse/orders
icetable config remove orders
icetable config list

# Manage catalogs
icetable config add-catalog nessie http://nessie:19120/api/v2
icetable config remove-catalog nessie
```

### catalog

Interact with REST catalogs directly.

```bash
icetable catalog -c nessie info
icetable catalog -c nessie namespaces
icetable catalog -c nessie -n analytics tables
icetable catalog -c nessie create-namespace analytics
icetable catalog -c nessie drop-namespace analytics --force
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

# Reproducible with seed
icetable generate -t /tmp/test --template events --rows 1000 --seed 42

# Append to existing table (will prompt for confirmation)
icetable generate -t /tmp/test --template events --rows 5000

# Append without confirmation
icetable generate -t /tmp/test --template events --rows 5000 --force
```

### Other commands

```bash
# Initialize new empty table
icetable init -t s3://warehouse/new_table --schema schema.json

# Compare snapshots
icetable diff --from 123 --to 456

# View history
icetable history

# Compute statistics
icetable stats

# Validate table integrity
icetable validate

# Import from Delta Lake
icetable import delta -t s3://warehouse/iceberg_table --source s3://warehouse/delta_table

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

## Output Formats

All commands support JSON output:

```bash
icetable inspect --output json
icetable snapshot list --output json | jq '.[].id'
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

### Daily maintenance

```bash
# Check health
icetable analyze

# Compact if needed
icetable optimize --dry-run
icetable optimize

# Expire old snapshots
icetable snapshot expire --older-than 7d --retain-last 5

# Clean orphans
icetable vacuum
```

### Debug table issue

```bash
icetable inspect --metadata
icetable snapshot list --all
icetable snapshot lineage

# Roll back
icetable snapshot set --id 1234567890123
```

### Work with catalog

```bash
# Setup
icetable config add-catalog prod http://nessie:19120/api/v2
icetable config use events prod.analytics.events

# Daily ops
icetable analyze
icetable optimize
icetable snapshot expire --older-than 30d
```

## License

Apache-2.0
