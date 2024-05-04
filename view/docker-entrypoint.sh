#!/bin/bash

_DB_LOCATION=${DB_LOCATION:-/mnt/volume/rocksdb}
_PROGRAM_PATH=${PROGRAM_PATH:-/mnt/volume/indexer.wasm}

echo "DB_LOCATION: $_DB_LOCATION"
echo "PROGRAM_PATH: $_PROGRAM_PATH"

/view/target/debug/metashrew-view-rs
