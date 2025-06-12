# Using rockshrew-mono

`rockshrew-mono` is the combined binary that integrates both the indexer and view layer in a single process. This document explains how to use `rockshrew-mono`, including command-line options, configuration, deployment scenarios, and performance tuning.

## Overview

`rockshrew-mono` provides several advantages over running separate indexer and view processes:

1. **Simplified Deployment**: Single binary to deploy and manage
2. **Shared Resources**: Efficient resource sharing between indexing and querying
3. **Improved Stability**: Avoids inter-process communication issues
4. **Consistent State**: Ensures the view layer always sees the latest indexed state

## Command-Line Options

`rockshrew-mono` supports the following command-line options:

```
USAGE:
    rockshrew-mono [OPTIONS] --daemon-rpc-url <URL> --indexer <FILE> --db-path <PATH>

OPTIONS:
    --daemon-rpc-url <URL>           URL of the Bitcoin Core RPC server
    --indexer <FILE>                 Path to the WASM indexer module
    --db-path <PATH>                 Path to the database directory
    --start-block <HEIGHT>           Block height to start indexing from (default: 0)
    --auth <USER:PASS>               Authentication for Bitcoin Core RPC
    --host <HOST>                    Host to bind the JSON-RPC server to (default: 127.0.0.1)
    --port <PORT>                    Port for the JSON-RPC server (default: 8080)
    --label <LABEL>                  Namespace label for the database
    --exit-at <HEIGHT>               Exit after indexing to this height
    --pipeline-size <SIZE>           Size of the processing pipeline
    --cors <ORIGINS>                 CORS allowed origins (e.g., '*' or 'domain1.com,domain2.com')
    --snapshot-interval <INTERVAL>   Interval in blocks between snapshots
    --snapshot-directory <PATH>      Directory to store snapshots
    --repo <URL>                     URL of snapshot repository to sync from
    --migrate-to-bst                 Migrate existing data to the height-indexed BST structure
    --migrate-start-height <HEIGHT>  Start height for BST migration (default: 0)
    --migrate-end-height <HEIGHT>    End height for BST migration (default: current height)
    -h, --help                       Print help information
    -V, --version                    Print version information
```

### Required Options

- `--daemon-rpc-url`: URL of the Bitcoin Core RPC server (e.g., `http://localhost:8332`)
- `--indexer`: Path to the WASM indexer module (e.g., `./indexer.wasm`)
- `--db-path`: Path to the database directory (e.g., `~/.metashrew`)

### Authentication

- `--auth`: Authentication for Bitcoin Core RPC in the format `username:password`

### Network Options

- `--host`: Host to bind the JSON-RPC server to (default: `127.0.0.1`)
- `--port`: Port for the JSON-RPC server (default: `8080`)
- `--cors`: CORS allowed origins (e.g., `*` or `domain1.com,domain2.com`)

### Indexing Options

- `--start-block`: Block height to start indexing from (default: `0`)
- `--exit-at`: Exit after indexing to this height
- `--pipeline-size`: Size of the processing pipeline (default: auto-determined based on CPU cores)
- `--label`: Namespace label for the database

### Snapshot Options

- `--snapshot-interval`: Interval in blocks between snapshots
- `--snapshot-directory`: Directory to store snapshots
- `--repo`: URL of snapshot repository to sync from

### BST Migration Options

- `--migrate-to-bst`: Migrate existing data to the height-indexed BST structure
- `--migrate-start-height`: Start height for BST migration (default: `0`)
- `--migrate-end-height`: End height for BST migration (default: current height)

## Basic Usage

### Starting the Indexer

To start indexing from the beginning of the blockchain:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --host 0.0.0.0 \
  --port 8080
```

### Starting from a Specific Height

To start indexing from a specific block height:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --start-block 500000
```

### Using a Namespace Label

To use a namespace label for the database:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --label my-indexer
```

### Creating Snapshots

To create snapshots at regular intervals:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --snapshot-interval 10000 \
  --snapshot-directory ~/.metashrew/snapshots
```

### Syncing from a Snapshot Repository

To sync from a snapshot repository:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --repo https://example.com/snapshots
```

### Migrating to Height-Indexed BST

To migrate existing data to the height-indexed BST structure:

```bash
rockshrew-mono \
  --daemon-rpc-url none \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --migrate-to-bst
```

## Configuration

### Environment Variables

`rockshrew-mono` supports the following environment variables:

- `HOST`: Host to bind the JSON-RPC server to (same as `--host`)
- `PORT`: Port for the JSON-RPC server (same as `--port`)
- `RUST_LOG`: Controls log verbosity (e.g., `info`, `debug`, `trace`)

Example:

```bash
RUST_LOG=debug HOST=0.0.0.0 PORT=8080 rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

### Logging

Log levels can be controlled using the `RUST_LOG` environment variable:

- `error`: Only show errors
- `warn`: Show warnings and errors
- `info`: Show info messages, warnings, and errors (default)
- `debug`: Show debug messages, info messages, warnings, and errors
- `trace`: Show all log messages

Example:

```bash
RUST_LOG=debug rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

### RocksDB Configuration

`rockshrew-mono` automatically configures RocksDB based on the available system resources. The configuration includes:

- **Max Open Files**: 10,000
- **Write Buffer Size**: 256 MB
- **Target File Size**: 256 MB
- **Background Jobs**: Auto-configured based on CPU cores
- **Write Buffer Number**: Auto-configured based on CPU cores

## Deployment Scenarios

### Local Development

For local development, you can run `rockshrew-mono` with default settings:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

### Production Deployment

For production deployment, consider the following:

1. **Bind to All Interfaces**: Use `--host 0.0.0.0` to allow external connections
2. **Restrict Access**: Use a firewall to restrict access to the JSON-RPC port
3. **Increase Pipeline Size**: Use `--pipeline-size` to optimize for your hardware
4. **Enable Snapshots**: Use `--snapshot-interval` and `--snapshot-directory` for data backup
5. **Use a Process Manager**: Use a process manager like systemd or Docker to manage the process

Example systemd service file:

```ini
[Unit]
Description=Metashrew Indexer
After=network.target

[Service]
User=metashrew
ExecStart=/usr/local/bin/rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer /opt/metashrew/indexer.wasm \
  --db-path /var/lib/metashrew \
  --host 0.0.0.0 \
  --port 8080 \
  --snapshot-interval 10000 \
  --snapshot-directory /var/lib/metashrew/snapshots
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### Docker Deployment

`rockshrew-mono` can be deployed using Docker:

```dockerfile
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY rockshrew-mono /usr/local/bin/
COPY indexer.wasm /opt/metashrew/

VOLUME /var/lib/metashrew

EXPOSE 8080

CMD ["rockshrew-mono", \
     "--daemon-rpc-url", "http://bitcoin:8332", \
     "--auth", "bitcoinrpc:bitcoinrpc", \
     "--indexer", "/opt/metashrew/indexer.wasm", \
     "--db-path", "/var/lib/metashrew", \
     "--host", "0.0.0.0", \
     "--port", "8080"]
```

Docker Compose example:

```yaml
version: '3'

services:
  bitcoin:
    image: ruimarinho/bitcoin-core:latest
    volumes:
      - bitcoin_data:/home/bitcoin/.bitcoin
    command:
      - -server
      - -rpcbind=0.0.0.0
      - -rpcallowip=0.0.0.0/0
      - -rpcuser=bitcoinrpc
      - -rpcpassword=bitcoinrpc
      - -txindex=1
    ports:
      - "8332:8332"

  metashrew:
    image: metashrew:latest
    volumes:
      - metashrew_data:/var/lib/metashrew
    ports:
      - "8080:8080"
    depends_on:
      - bitcoin
    environment:
      - RUST_LOG=info

volumes:
  bitcoin_data:
  metashrew_data:
```

### SSH Tunneling

`rockshrew-mono` supports SSH tunneling for connecting to a remote Bitcoin Core node:

```bash
rockshrew-mono \
  --daemon-rpc-url ssh://user@remote-host:22/http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

This will establish an SSH tunnel to `remote-host` and forward connections to the Bitcoin Core RPC server running on the remote host.

## Performance Tuning

### Pipeline Size

The pipeline size controls how many blocks are processed in parallel. By default, it's auto-determined based on CPU cores, but you can override it:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --pipeline-size 16
```

Larger pipeline sizes can improve throughput but require more memory.

### Memory Management

`rockshrew-mono` automatically manages memory for the WASM runtime. If you encounter memory issues, consider:

1. **Reducing Pipeline Size**: Use a smaller pipeline size to reduce memory usage
2. **Increasing System Memory**: Ensure your system has enough memory for the workload
3. **Monitoring Memory Usage**: Use tools like `htop` to monitor memory usage

### Disk I/O

RocksDB performance is heavily dependent on disk I/O. Consider:

1. **Using SSDs**: SSDs provide much better random I/O performance than HDDs
2. **RAID Configuration**: Consider RAID 0 or RAID 10 for improved I/O performance
3. **File System Tuning**: Tune your file system for database workloads (e.g., disabling atime updates)

### Network Bandwidth

If you're running Bitcoin Core on a different machine, ensure you have sufficient network bandwidth between the machines.

### CPU Utilization

`rockshrew-mono` is designed to utilize multiple CPU cores efficiently. Consider:

1. **CPU with High Single-Thread Performance**: WASM execution benefits from high single-thread performance
2. **Multiple CPU Cores**: More cores allow for better parallelization of block processing
3. **CPU Frequency**: Higher CPU frequency generally leads to better performance

## Monitoring

### Log Monitoring

Monitor the logs for important information:

```bash
tail -f ~/.metashrew/logs/metashrew.log
```

### Process Monitoring

Use tools like `htop` or `ps` to monitor the process:

```bash
htop -p $(pgrep rockshrew-mono)
```

### Database Size Monitoring

Monitor the size of the database directory:

```bash
du -sh ~/.metashrew
```

### JSON-RPC Monitoring

You can monitor the JSON-RPC server by periodically querying the `metashrew_height` method:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"metashrew_height","params":[]}' http://localhost:8080
```

## Troubleshooting

### Common Issues

1. **Connection Refused**: Ensure Bitcoin Core is running and accessible
2. **Authentication Failed**: Check the `--auth` parameter
3. **Permission Denied**: Check file permissions for the database directory
4. **Out of Memory**: Reduce pipeline size or increase system memory
5. **Slow Indexing**: Check disk I/O, network bandwidth, and CPU utilization

### Error Messages

1. **"Failed to connect to Bitcoin Core"**: Check the `--daemon-rpc-url` parameter and ensure Bitcoin Core is running
2. **"Authentication failed"**: Check the `--auth` parameter
3. **"Failed to open database"**: Check the `--db-path` parameter and ensure the directory exists and is writable
4. **"Failed to load WASM module"**: Check the `--indexer` parameter and ensure the file exists and is a valid WASM module
5. **"Out of memory"**: Reduce pipeline size or increase system memory

### Recovering from Errors

If `rockshrew-mono` crashes or is terminated unexpectedly, it will automatically resume indexing from the last processed block when restarted.

## Conclusion

`rockshrew-mono` provides a powerful and flexible way to run Metashrew indexers. By understanding the command-line options, configuration, deployment scenarios, and performance tuning, you can optimize your deployment for your specific needs.

Whether you're running a simple local indexer or a production deployment, `rockshrew-mono` provides the tools you need to efficiently index and query Bitcoin blockchain data.