#!/bin/bash
export RUST_LOG=debug 
ulimit -n $(ulimit -n -H)
/data/metashrew/target/release/rockshrew-mono --db-path /data/db --daemon-rpc-url http://localhost:8332 --auth bitcoinrpc:bitcoinrpc --indexer /data/alkanes-rs/target/wasm32-unknown-unknown/release/alkanes.wasm --host 0.0.0.0 --port 8080
