# metashrew

An Electrum Server fork with Bitcoin indexing logic swapped out with a minimalistic WASM runtime, with host bindings to get and set rocksdb key-value pairs, as well as convenient data structures.

## Install

```sh
git clone https://github.com/sandshrewmetaprotocols/metashrew
cd metashrew
cargo build
```

## Usage

All flags permitted by the canonical `electrs` program (available at [https://github.com/romanz/electrs](https://github.com/romanz/electrs)) can be used to invoke a metashrew process.

The `--indexer <wasm file>` flag can be passed to supply the WASM file that runs your indexer or metaprotocol logic.

To scaffold indexer projects that can be run within metashrew, it is convenient to use this tool:

[https://github.com/sandshrewmetaprotocls/metashrew-cli](https://github.com/sandshrewmetaprotocols/metashrew-cli)

Invoke metashrew with a command such as:

```sh
./target/debug/metashrew --db-dir ~/.metashrew --daemon-dir ~/.bitcoin --network bitcoin --electrum-rpc-addr 127.0.0.1:35556 --daemon-rpc-addr 127.0.0.1:8332 --daemon-p2p-addr 127.0.0.1:8333 --monitoring-addr 127.0.0.1:4225 --indexer ~/dns-on-bitcoin/build/release.wasm
```

metashrew will begin syncing chaindata over the p2p socket (very quickly) and the WASM program will be instantiated and executed with the blockdata copied to its memory export. In this way, the WASM program receives a callback for each block metashrew processes, from which you can parse the block height from the first four bytes (little-endian) then parse the full serialized block which follows.

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
