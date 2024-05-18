#!/bin/bash

_DAEMON_RPC_ADDR=${DAEMON_RPC_ADDR:-127.0.0.1:8332}
_DAEMON_P2P_ADDR=${DAEMON_P2P_ADDR:-127.0.0.1:8333}
_NETWORK=${NETWORK:-mainnet}
_INDEXER=${INDEXER:-/metashrew/indexer.wasm}
_DAEMON_DIR=${DAEMON_DIR:-/bitcoin}
_DB_DIR=${DB_DIR:-/metashrew/rocksdb}
_LOG_FILTERS=${LOG_FILTERS:-DEBUG}
_ELECTRUM_RPC_ADDR=${ELECTRUM_RPC_ADDR:-127.0.0.1:50009}
_MONITORING_ADDR=${MONITORING_ADDR:-127.0.0.1:34442}
_INDEX_BATCH_SIZE=${INDEX_BATCH_SIZE:-2000}

echo "waiting for $_INDEXER to appear ..."
while [ ! -f $_INDEXER ]; do
  sleep 5
done

/opt/metashrew/target/release/metashrew --log-filters $_LOG_FILTERS --db-dir $_DB_DIR --daemon-dir $_DAEMON_DIR --network $_NETWORK --daemon-rpc-addr $_DAEMON_RPC_ADDR --daemon-p2p-addr $_DAEMON_P2P_ADDR --electrum-rpc-addr ${_ELECTRUM_RPC_ADDR} --monitoring-addr $_MONITORING_ADDR --indexer $_INDEXER
