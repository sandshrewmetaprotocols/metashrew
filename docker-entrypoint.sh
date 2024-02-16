#!/bin/bash

_DAEMON_RPC_ADDR=${DAEMON_RPC_ADDR:-bitcoind-nfs-service.default.svc.cluster.local:8332}
_NETWORK=${NETWORK:-mainnet}
_INDEXER=${INDEXER:-/metashrew/indexer.wasm}
_DAEMON_DIR=${DAEMON_DIR:-/bitcoin}
_DB_DIR=${DB_DIR:-/metashrew/rocksdb}
_LOG_FILTERS=${LOG_FILTERS:-DEBUG}

/electrs/target/debug/metashrew --log-filters $_LOG_FILTERS --db-dir $_DB_DIR --daemon-dir $_DAEMON_DIR --network $_NETWORK --daemon-rpc-addr $_DAEMON_RPC_ADDR --electrum-rpc-addr 127.0.0.1:50009 --monitoring-addr 127.0.0.1:34442 --indexer $_INDEXER
