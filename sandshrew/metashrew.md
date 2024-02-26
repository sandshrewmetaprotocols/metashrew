# Metashrew

## Overview

Metashrew is a project inspired by the electrs project, originally designed for efficiently indexing Bitcoin chaindata to serve an Electrum wallet instance. However, Metashrew takes indexing to a new level by introducing a WebAssembly (WASM) environment that empowers developers to supply custom WASM programs to execute indexing logic securely. The function of metashrew is to allow developers to build extensions on top of sandshrew, becoming the foundation in your next project.

Key Features of Metashrew:

1. **WASM-Based Indexing:** Metashrew replaces traditional indexing logic with a WebAssembly (WASM) environment. Developers can provide their own WASM programs to execute complex indexing tasks securely within this environment.
2. **Custom View Functions:** The same WASM program used for indexing can expose custom view functions. These functions are made accessible through a JSON-RPC interface hosted on the same process, allowing developers to implement queries that retrieve data indexed during the indexing process.
3. **Efficient Block Processing:** Metashrew efficiently processes Bitcoin blocks by synchronizing with them over the peer-to-peer (p2p) socket exposed by a local bitcoind instance. This approach offers significant efficiency gains compared to traditional ordinals indexers, which rely on bitcoind RPC for synchronization.
4. **Archived Block State:** At the end of each block, the WASM program generates an RLP encoded set of key/value pairs representing updates to the index. Additionally, the state of the index for each block is archived. This archival approach enables querying earlier blocks to retrieve historical state information.
5. **Assemblyscript Library:** Metashrew includes an Assemblyscript library designed for top-level indexing of Bitcoin blocks, with the capability to parse ordinals inscriptions. This library can be leveraged to create various metaprotocols and extensions.

## Metaprotocol development on sandshrew

Metashrew is a dynamic and adaptable indexer tailored for Bitcoin metaprotocols. While the initial development efforts have concentrated on indexing ordinals through [metashrew-ord](metashrew.md#metashrew-ord), our vision extends to encompass various Metashrew indexers, including those designed for BRC-20s and other emerging meta-protocols.

With a great lack of flexible indexer frameworks for Bitcoin, Metashrew is designed to fill the gap. Metaprotocols can be quickly prototyped using the Metashrew framework and built to wasm, such that they can be fully hosted as a service by the sandshrew system, in a reliable cloud hosted environment with proper failover.

Mainnet.sandshrew.io and testnet.sandshrew.io exist

## Metashrew-view

Metashrew-view is a parallelizable view layer for metashrew designed to enhance the scalability of WebAssembly instances without scaling Read/Write performance of rocksdb. This will allow developers the ability to expose the metashrew\_view JSON-RPC method and can be reverse proxied by the complete RPC provider to surface data for a single index.\


Download and Install Metashrew-view from the sandshrew repository.

```
git clone https://github.com/sandshrew/metashrew-view
cd metashrew-view
yarn
npm install -g
```

Set environment variables HOST, PORT, DB\_LOCATION and PROGRAM\_PATH then run the command

```
metashrew-view
```

This will run the HTTP service and can be reverse proxied.

The remote rocksdb database must be mounted over FUSE, nfs, or something similar that exposes native filesystem access to the remote index. The database is opened in read-only mode and a handle is instantiated for every call to metashrew\_view. It is expected that a metashrew process will own the rocksdb volume where the index is built. This same filesystem mount is intended to be provided over the network to metashrew-view such that it may be opened in read-only mode.

The metashrew\_view RPC call takes three params:

```
[ programHash, functionName, inputHexString, blockTag ]
```

It returns a hex encoded byte string as the JSON-RPC result.

## Metashrew-ord

Metashrew-ord is engineered with minimal overhead in mind, aiming for a near-zero-copy design philosophy, following the footsteps of the electrs indexer routines, from which Metashrew is dervied. Within this project, the AssemblyScript serves as a versatile template for crafting WebAssembly programs capable of indexing various Bitcoin block data within the Metashrew environment. To assist developers in this endeavor, a test harness is provided, enabling them to execute a simulated WebAssembly runtime that replicates the calling conventions of the production Metashrew runtime. This setup is ideal for constructing custom indexers and tailored Metaprotocols, offering both flexibility and efficiency.

Download and install Metashrew-ord from the sandshrew repository.

```
git clone http://18.191.184.3:3000/sandshrew/metashrew-ord
npm install
```

You can then run local tests that will parse example blocks from the tests directory.

```
npm run test
```
