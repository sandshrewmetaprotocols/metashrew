# metashrew

Bitcoin indexer framework with WASM VM.

Metashrew was architected to reduce the problem of building a metaprotocol, or even a lower-level indexer, to architecting an executable that handles solely the logic of processing a single block. The executables follow a portable calling convention and are compatible with AssemblyScript, but can also, themselves, be written in C or Rust, or anything that builds to a WASM target.

Repositories are hosted within [https://github.com/sandshrewmetaprotocols](https://github.com/sandshrewmetaprotocols) with a number of metaprotocols already built to WASM, which can be loaded into metashrew.

## Install

```sh
git clone https://github.com/sandshrewmetaprotocols/metashrew
cd metashrew
cargo build
```

## Usage

metashrew has two functional builds which may be deployed depending on the context. The metashrew/index directory contains sources for the electrs fork that runs metashrew, and the metashrew/keydb directory contains sources for a standalone metashrew which connects over a generic RPC socket and thus offers more compatibility. The keydb build is, in most cases, the appropriate process to run to begin indexing, if the environment has access to a keydb instance running on the same host with efficient disk.

### electrs

All flags permitted by the canonical `electrs` program (available at [https://github.com/romanz/electrs](https://github.com/romanz/electrs)) can be used to invoke a metashrew process.

```sh
./target/debug/metashrew --db-dir ~/.metashrew --daemon-dir ~/.bitcoin --network bitcoin --electrum-rpc-addr 127.0.0.1:35556 --daemon-rpc-addr 127.0.0.1:8332 --daemon-p2p-addr 127.0.0.1:8333 --monitoring-addr 127.0.0.1:4225 --indexer ~/dns-on-bitcoin/build/release.wasm
```

When invoked, metashrew will begin syncing chaindata over the p2p socket (very quickly) and the WASM program will be instantiated and executed with the blockdata copied to its memory export. In this way, the WASM program receives a callback for each block metashrew processes, from which you can parse the block height from the first four bytes (little-endian) then parse the full serialized block which follows.

### metashrew-keydb

The keydb process generally performs better since we can assume there is a long-lived rocksdb backed process available in the environment and we are not required to flush to disk synchronously with every batch of k/v pairs.

To run metashrew-keydb, it is first necessary to deploy a keydb instance on the host, since the best performance is achieved over a UNIX socket. A redis URI can also be used to connect over TCP, if deploying to a cluster environment.

A generic docker-compose.yaml file is provided in metashrew/keydb/docker-compose.yaml which is usable if an .env file is created at metashrew/keydb/.env and contents are included similar to:

```
KEYDB_DATA=/home/ubuntu/keydb
```

The default docker-compose.yaml file will instantiate a UNIX socket at <KEYDB_DATA>/keydb.sock which, at least on the Linux implementation of docker-ce, can be used transparently through the mounted container volume.

An example invocation of metashrew-keydb is produced below:

```sh
./metashrew/target/debug/metashrew-keydb --indexer ~/metashrew-runes/build/debug.wasm --start-block 840000 --label mainnet-runes --redis redis+unix:///home/ubuntu/keydb/keydb.sock --daemon-rpc-url http://localhost:8332 --auth bitcoinrpc:bitcoinrpc
```

The `--label` flag is optional and defines a prefix to use when inserting k/v pairs into keydb. If mainnet-runes is supplied as a label, keys will be prefixed with `mainnet-runes://` followed by the key bytes. This makes it possible to use the same keydb instance as a backend to build many indexes at once. We are locked to a single thread since we expect indexers be required to process blocks in series, so it is often economical to build multiple separate indexes in parallel, depending on your needs.

The `--indexer <wasm file>` flag can be passed to supply the WASM file that runs your indexer or metaprotocol logic.

The `--start-block <block height>` flag is only honored during an indexer first run. If an index has already begun building, it will always use the block number that it left off at to continue.

The `--auth` flag refers to the authentication string needed for RPC access, and is optional.

#### HINT: clearing indexes built under a label

You can use the redis API to run LUA scripts the same as on the canonical version of redis.

Use this command to clear a subset of the k/v cache which is labeled with mainnet-runes, while keeping other indexes intact.

```sh
cd metashrew/keydb
docker-compose exec keydb redis-cli
# drops to a redis shell
EVAL "local cursor = 0 local calls = 0 local dels = 0 repeat    local result = redis.call('SCAN', cursor, 'MATCH', ARGV[1])     calls = calls + 1   for _,key in ipairs(result[2]) do       redis.call('DEL', key)      dels = dels + 1     end     cursor = tonumber(result[1]) until cursor == 0 return 'Calls ' .. calls .. ' Dels ' .. dels" 0 mainnet-runes://*:1
```

Be sure you stop indexer processes before running this since connections will break during atomic execution of the script.

Credits for this LUA to Stackoverflow answer here: [https://stackoverflow.com/questions/61366419/how-to-atomically-delete-millions-of-keys-matching-a-pattern-using-pure-redis](https://stackoverflow.com/questions/61366419/how-to-atomically-delete-millions-of-keys-matching-a-pattern-using-pure-redis)


### metashrew-keydb-view

The metashrew indexer process runs separately from the `view` process. The metashrew-keydb-view process provides RPC access to view function exports of the loaded WASM binary, where functions can be called that do not effect state changes on the index. Any state changes during the execution of a view function only persist for the duration of the function call, sandboxed within the context of the function. Any exported function of the WASM program can be invoked via the RPC.

Sample invocation of the metashrew-keydb-view process:

```sh
export RUST_LOG=DEBUG
export REDIS_URI=redis+unix:///home/ubuntu/keydb/keydb.sock
export PROGRAM_PATH=/home/ubuntu/metashrew-runes/build/debug.wasm
export REDIS_LABEL=mainnet-runes
export PORT=8080
./metashrew/target/debug/metashrew-keydb-view 
```

Then to invoke a view function, an example curl command could look like

```sh
curl http://localhost:8080 -H 'Content-Type: application/json' -d '{"jsonrpc": "2.0", "id": 0, "method": "metashrew_view", "params": ["outpoint", "0x0a20e5c74c1cffbc96e3d825635a9dd0ea16f99d1f240e2ab076eb88fea9ac6de64f", "latest"]}'
```

The encoding of the bytearray passed as the second parameter is arbitrary. In this example it is a protobuf encoded outpoint as defined in the file [https://github.com/sandshrewmetaprotocols/metashrew-runes/tree/master/proto/metashrew-runes.proto](https://github.com/sandshrewmetaprotocols/metashrew-runes/tree/master/proto/metashrew-runes.proto).

This is only the case because the view function implementation specifically decodes with an AssemblyScript implementation of protobuf. Any serialization format or language choice (which has WASM build support) could be used for this view function.


## Scaffolding indexer programs

To scaffold indexer projects that can be run within metashrew, it is convenient to use this tool:

[https://github.com/sandshrewmetaprotocls/metashrew-cli](https://github.com/sandshrewmetaprotocols/metashrew-cli)


## Docker

The provided docker-compose.yaml is currently only provided for the electrs build of metashrew. The keydb build will be provided in a future update, as a pair of docker images (indexer/view).
Create an .env file in the root of the project with the variables

```
METASHREW_DATA=<directory where metashrew data should live>
BITCOIN_DATA=<directory where bitcoind writes to>
```

metashrew will wait for the presernce of a `<metashrew directory/indexer.wasm` file before it will initialize, so it is acceptable to define your directories via the .env and then copy the WASM file from a project such as [https://github.com/sandshrewmetaprotocols/metashrew-ord](https://github.com/sandshrewmetaprotocols/metashrew-ord) into that directory, then check the logs to look at progress.

A suitable command to check logs is;

```sh
cd metashrew
docker-compose logs -f --tail 100

```

You will see sync updates here.

Additional variables can be supplied from an .env file. For the complete set refer to the docker-compose.yaml file.

## Runtime

Host functions are made available to the WASM program via the `env` module.

To implement a runtime compatible with metashrew, if the ones within this Rust project, metashrew-view, or metashrew-test are not suitable for your use case, it is required to implement the functions listed below.

- `__log(i32)`
- `__load_input(i32)`
- `__host_len()`
- `__flush(i32)`
- `__get(i32, i32)`
- `__get_len(i32): i32`

All i32 parameters mentioned here MUST represent a pointer to the location in the WebAssembly memory region where memory should be written or read from. All pointers follow the same memory layout as an `ArrayBuffer` type from the AssemblyScript, though it is possible to implement a runtime that can run WASM programs without AssemblyScript. The only requirement is that the byte size of the region that the pointer points to is stored as a `u32` (little-endian) in the 4-byte region immediately preceding the location in memory that the pointer references.

The host function `__host_len(): i32` must return an i32 representing the length of the region that the WASM program should allocate, via any means, in memory, to store the block height (as a little-endian u32) + the serialized block. This function is called, ideally, at the start of the WASM run for each block, immediately before calling `__load_input(i32)` which must be called with a pointer to the region that has been allocated by the WASM program.

For a `__load_input(i32)` implementation, it is required to copy the bytes representing either the view function input (for use with metashrew-view read-only functions for the index) or otherwise a byte vector composed of a little-endian u32 representing the block height of the block being processed, followed by the serialized block.

An example implementation within a WASM program that makes use of the convention to load the program input is available in metashrew-as, but is reproduced below for reference:

```js
function input(): ArrayBuffer {
  const data = new ArrayBuffer(__host_len());
  __load_input(data);
  return data;
}
```

A `__log(i32)` implementation MUST use the host environment to print the bytes (which must be encoded as UTF-8) to stdout. Input is a pointer to an `ArrayBuffer` compatible data structure in WASM memory.

A `__flush(i32)` implementation MUST decode its `ArrayBuffer` compatible input with an RLP decoder and handle a 1-D list of byte vectors, flattened from a list of `[key, value]` pairs to a single `[key, value, key, value, key, value, ...]` list. The host environment MUST store key-value pairs in its database backend such that they can be accessed historically as the WASM program is run again for each new block.

A `__get_len(i32)` implementation MUST return the size of the memory region needed to store the result of a key-value lookup for the key supplied as an argument (as an `ArrayBuffer`)

A `__get(i32, i32)` implementation MUST use the byte vector supplied as its first parameter (as an `ArrayBuffer`) to do a key-value lookup, and the result of the lookup MUST be copied to the region of memory pointed to by the second parameter.

An example implementation of a general purpose `get(key: ArrayBuffer)` function can be seen in the metashrew-as repository, but a trivial implementation that does not perform any caching is reproduced below, to aid understanding of the workflow for a key-value lookup from the WASM program:

```js
function get(k: ArrayBuffer): ArrayBuffer {
  let result = new ArrayBuffer(__get_len(k));
  __get(k, result);
  return result;
}
```

#### AssemblyScript compatibility

For running an AssemblyScript program within a custom runtime, the AssemblyScript runtime can be included in your build but it will be necessary to implement a trivial

`abort(): void`

As a host function. In the Rust implementation of metashrew, it simply calls `panic!("abort");`

If this host function is not included, a runtime will not run the AssemblyScript WASM, which expects it.

## Database

The rocksdb database built by a metashrew process is designed to be append-only, but this detail is abstracted for developers designing a program for metashrew. Under the hood, instead of overwriting a key-value pair, when the same key is written to by a host call from the WASM instance to the `__set` function, a list structure is appended to in the underlying database where the key is annotated by the position in the list maintained for that key, and the value is annotated with the block height at which the value was updated. In this way, it is trivial to rollback updates to the key-value store, by maintaining a list of touched keys for each new block, then in the event of a reorg, those keys can be procedurally rolled back to the block prior to the divergence in the chain, and index updates can be applied for the blocks which are now accepted as valid.

Additionally, this append-only structure makes it possible to write view functions for the index that can run archived state, to check the state of the index at the desired block height.

For specific implementation details, refer to `src/index.rs`

## View Functions

metashrew extends the electrs JSON-RPC (available over a TCP socket) with `metashrew.view`

View functions can be included in the WASM program and exported under any name. It is expected that a view function implementation will use the same convention to read the host input as the `_start` function is expected to when it reads the block height and serialized block.

View functions have the exact same runtime, except the `__flush` function MUST be a no-op, to ensure that the WASM program can be run in read-only mode without side-effects on the database.

The parameters to the RPC call to invoke view functions follow the format:

```js
[functionName, inputAsHex, blockTag]
```

`blockTag` must be either the string `latest` or a block number, represented as hex.

## Author


