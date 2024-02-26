# Sandshrew Indexer

## Overview

Sandshrew is an open source Bitcoin indexer that enables developers to integrate bitcoin [BRC-20s](../guides/brc-20s.md), [ordinals](../guides/ordinals.md) and [inscriptions](../guides/inscriptions.md) into your applications. It provides everything needed to develop and exchange ordinals, build inscription services, and create Bitcoin transactions.

Sandshrew is comprised of three RPC services. The [Bitcoin Core RPC](bitcoin-core-rpc.md) provides developers with access to a full Bitcoin node. All Bitcoin Core methods use the `btc_` namespace and follow the standard BTC RPC specification. &#x20;

```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getblock",
    "params": [
      "00000000c937983704a73af28acdec37b049d214adbda81d7e2a3dd146f6ed09",
      1
    ]
}'
```

The [Esplora RPC](esplora-rpc.md) allows you to easily query transaction histories, block information, and address balances. All Esplora methods use the `esplora_` namespace and follow the [Esplora REST API](https://github.com/Blockstream/electrs/blob/new-index/src/rest.rs#L640). The `esplora_` namespace partitions the part of the method name following the \_ character by the `:` character. An empty space between `:` characters is where each string in the Sandshrew `params` is inserted, sequentially.

For example, this Esplora REST endpoint:

```url
https://blockstream.info/api/address/bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7/txs
```

becomes this Sandshrew RPC call:

```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::txs",
    "params": [
      "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7"
    ]
}'
```

The [Ordinal RPC](ordinals-rpc.md) provides features that enable users to create inscriptions and interact with their ordinals. All Ordinal methods use the `ord_` namespace and follow the REST API implemented by the [ordinal indexer](https://github.com/ordinals/ord/blob/master/src/subcommand/server.rs#L186).

The Ordinal REST endpoints translate to JSON-RPC such that the hardcoded component of the REST path is the suffix to the `ord_*` JSON-RPC method, and the `params` field of the JSON-RPC POST body contains all query parameters that the given `ord` route accepts.

For example, this REST endpoint:

```url
https://ordinals.com/inscription/640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59i0
```

becomes this Sandshrew RPC call:

```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_inscription",
    "params": [
      "640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59i0"
    ]
}'
```

## Building the Docker Image

Sandshrew can be downloaded from the Oyl-Wallet repository.

```
git clone https://github.com/oyl-wallet/sandshrew
```

Once Docker is up and running, you can build and run Sandshrew in different deployment modes.

### Running Sandshrew for Regtest

To build sandshrew for a local `regtest` environment, first build the docker file:

```
docker build -t sandshrew:regtest -f Dockerfile-regtest .
```

and then run it:

```
docker run -it -p 3000:3000 sandshrew:regtest
```

The local Sandshrew RPC will be available at:

```
http://localhost:3000/v1
```

## Resources

* [Bitcoin Developer RPC API Reference](https://developer.bitcoin.org/reference/rpc/index.html)
