# Rockshrew Rollback Tool

A utility for rolling back a rockshrew database to a specific block height. This allows you to revert indexed data and restart indexing from an earlier point.

## Use Cases

- **Fixing corrupted data**: If blocks were indexed incorrectly due to a bug
- **Recovering from reorgs**: Roll back before a chain reorganization occurred
- **Testing**: Reset to a known state for testing purposes
- **Upgrading indexer**: Roll back before applying indexer code changes

## Installation

The rollback tool is built as part of the rockshrew-mono package:

```bash
cargo build --release -p rockshrew-mono --bin rockshrew-rollback
```

The binary will be located at:
```
target/release/rockshrew-rollback
```

## Usage

### Basic Syntax

```bash
rockshrew-rollback --db-path <PATH> --height <HEIGHT>
```

### Example

Roll back the database to block 925000:

```bash
rockshrew-rollback \
  --db-path /data/.metashrew-v9.0.2-v2.1.6 \
  --height 925000
```

This will:
1. Remove all indexed data for blocks 925001 and above
2. Update the indexed height marker to 925000
3. Allow rockshrew-mono to resume indexing from block 925001

### Options

- `--db-path <PATH>`: Path to the RocksDB database directory (required)
- `--height <HEIGHT>`: Target block height to roll back to (required)
- `--yes`: Skip the confirmation prompt (dangerous, use with caution)

## Safety Precautions

### Before Running

1. **Stop rockshrew-mono**: Ensure the indexer is NOT running
   ```bash
   # Check if running
   ps aux | grep rockshrew-mono

   # Stop if running
   pkill rockshrew-mono
   # or use your service manager (systemd, docker, etc.)
   ```

2. **Backup your database**: Always backup before rollback
   ```bash
   # Create backup
   cp -r /data/.metashrew-v9.0.2-v2.1.6 /data/.metashrew-v9.0.2-v2.1.6.backup

   # Or use rocksdb backup
   # See: https://github.com/facebook/rocksdb/wiki/How-to-backup-RocksDB%3F
   ```

3. **Check current height**: Verify the current indexed height
   ```bash
   # The tool will show this automatically when you run it
   ```

### Interactive Confirmation

By default, the tool prompts for confirmation:

```
⚠️  WARNING: This operation will modify the database in-place!
⚠️  Make sure rockshrew-mono is NOT running!
⚠️  Consider backing up your database first!

Type 'yes' to proceed with rollback:
```

Type `yes` and press Enter to proceed.

### Skip Confirmation (Not Recommended)

For automated scripts, you can skip the confirmation with `--yes`:

```bash
rockshrew-rollback --db-path /data/.metashrew --height 925000 --yes
```

**WARNING**: This skips all safety checks. Use only in controlled environments.

## What Gets Rolled Back

The rollback operation removes:

1. **Metadata keys**: Block hashes, state roots for heights > rollback height
2. **SMT data structures**: All indexed data stored via IndexPointer
3. **Height marker**: Updated to the rollback height

Specifically:
- Keys like `block_hash_925001`, `block_hash_925002`, etc. are deleted
- Keys like `state_root_925001`, `smt:root:925001`, etc. are deleted
- SMT append-only structures (`key/length`, `key/0`, `key/1`, etc.) are compacted
- Internal height marker (`__INTERNAL/height`) is set to the rollback height

## After Rollback

Once rollback completes:

```
✅ Rollback completed successfully!
Database has been rolled back from height 930000 to height 925000
You can now restart rockshrew-mono to resume indexing from block 925001
```

### Restart Indexing

Simply start rockshrew-mono normally:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --indexer /path/to/indexer.wasm \
  --db-path /data/.metashrew-v9.0.2-v2.1.6 \
  --host 0.0.0.0 \
  --port 8080
```

The indexer will:
- Detect the height is 925000
- Resume indexing from block 925001
- Continue forward normally

## Error Handling

### Database is Locked

```
Error: Failed to open database. Is rockshrew-mono still running?
```

**Solution**: Stop rockshrew-mono first.

### Invalid Rollback Height

```
Error: Rollback height (930000) must be less than current height (925000)
```

**Solution**: You can only roll back to a height less than the current indexed height.

### Database Doesn't Exist

```
Error: Database path does not exist: /data/.metashrew-v9.0.2-v2.1.6
```

**Solution**: Check the database path is correct.

## Example Workflow

Complete example of rolling back and resuming:

```bash
# 1. Check current state
ps aux | grep rockshrew-mono  # Ensure indexer is running

# 2. Stop the indexer
pkill rockshrew-mono

# 3. Backup database
cp -r /data/.metashrew-v9.0.2-v2.1.6 /data/.metashrew-v9.0.2-v2.1.6.backup

# 4. Run rollback
rockshrew-rollback \
  --db-path /data/.metashrew-v9.0.2-v2.1.6 \
  --height 925000

# Output shows:
# Current indexed height: 930000
# This will REMOVE data for 5000 blocks (925001 to 930000)
# Type 'yes' to proceed: yes
# ✅ Rollback completed successfully!

# 5. Restart indexer
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --indexer /path/to/indexer.wasm \
  --db-path /data/.metashrew-v9.0.2-v2.1.6 \
  --host 0.0.0.0 \
  --port 8080

# Indexer resumes from block 925001
```

## Verification

After rollback, you can verify the state:

```bash
# Check the height was updated
# (requires rockshrew-mono API to be running)
curl http://localhost:8080/height

# Should return: 925000
```

## Technical Details

### Rollback Algorithm

The rollback operation:

1. **Opens database read-only** to check current state
2. **Validates rollback height** < current height
3. **Opens database for writing** with exclusive lock
4. **Calls `rollback_smt_data()`** which:
   - Deletes metadata keys for heights > rollback height
   - Compacts SMT append-only structures by removing entries with heights > rollback height
   - Re-indexes remaining entries with sequential indices
5. **Updates height marker** to rollback height
6. **Verifies** the new height was set correctly

### Database Format

Rockshrew uses RocksDB with the following key structure:

**Internal keys:**
- `__INTERNAL/height`: Current indexed height (4 bytes, little-endian)
- `__INTERNAL/height-to-hash/{height}`: Block hash at height

**Metadata keys:**
- `block_hash_{height}`: Block hash (legacy format)
- `state_root_{height}`: State root hash
- `smt:root:{height}`: SMT root at height

**SMT data structures:**
- `{key}/length`: Number of updates for this key
- `{key}/{index}`: Update entry, format: `{height}:{hex_data}`

### Implementation

The rollback logic is implemented in:
- `crates/metashrew-runtime/src/rollback.rs` - Core rollback algorithm
- `crates/rockshrew-runtime/src/storage_adapter.rs` - RocksDB SmtRollback trait impl
- `crates/rockshrew-mono/src/bin/rollback.rs` - CLI tool

## See Also

- [Rollback implementation](../metashrew-runtime/src/rollback.rs)
- [Reorg handling](../../REORG_ROLLBACK_FIX.md)
- [SMT append-only design](../../SPECIFICATION.md)
