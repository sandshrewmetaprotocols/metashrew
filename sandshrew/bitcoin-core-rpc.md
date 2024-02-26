# Bitcoin Core RPC

Sandshrew includes a Bitcoin Core indexer. Bitcoin Core’s RPC API is a low level API maintained by Bitcoin Core developers. Sandshrew operates as a full Bitcoin node, and indexes the entire blockchain, encompassing all bitcoin transactions. This comprehensive indexing process ensures the integrity and accuracy of the data, making the Sandshrew indexer a reliable and essential tool for accessing and analyzing Bitcoin's complete transaction history and network data.

### Sending Transactions to the Bitcoin RPC Service

Interfacing with the Ordinals RPC service requires developers to [connect with Oyl and get a developer key](../welcome/getting-started.md).

All interactions with the Oyl Developer API follow a [standard RPC Query format](sandshrew-rpc.md).

## Blockchain RPCs

### getblock

Returns block information.

{% tabs %}
{% tab title="curl" %}
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
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**blockhash**: string (required)

**verbosity**: number (optional)

* 0 for hex-encoded data
* 1 for a json object (_default_)
* 2 for json object with transaction data&#x20;

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "hash": "00000000c937983704a73af28acdec37b049d214adbda81d7e2a3dd146f6ed09",
    "confirmations": 817296,
    "height": 1000,
    "version": 1,
    "versionHex": "00000001",
    "merkleroot": "fe28050b93faea61fa88c4c630f0e1f0a1c24d0082dd0e10d369e13212128f33",
    "time": 1232346882,
    "mediantime": 1232344831,
    "nonce": 2595206198,
    "bits": "1d00ffff",
    "difficulty": 1,
    "chainwork": "000000000000000000000000000000000000000000000000000003e903e903e9",
    "nTx": 1,
    "previousblockhash": "0000000008e647742775a230787d66fdf92c46a48c896bfbc85cdc8acc67e87d",
    "nextblockhash": "00000000a2887344f8db859e372e7e4bc26b23b9de340f725afbf2edb265b4c6",
    "strippedsize": 216,
    "size": 216,
    "weight": 864,
    "tx": [
      "fe28050b93faea61fa88c4c630f0e1f0a1c24d0082dd0e10d369e13212128f33"
    ]
  },
  "error": null,
  "id": 1
}
```

</details>

### getbestblockhash

Returns the hash of the best (tip) block in the most-work fully-validated chain.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getbestblockhash",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

```
{
  "result": "0000000000000000000015d6cd7fe70e544f403df9c5c0b7504c2fe32a0a4e2c",
  "error": null,
  "id": 1
}
```

</details>

### getblockchaininfo

Returns an object containing various state info regarding blockchain processing.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getblockchaininfo",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "chain": "main",
    "blocks": 819942,
    "headers": 819942,
    "bestblockhash": "0000000000000000000015d6cd7fe70e544f403df9c5c0b7504c2fe32a0a4e2c",
    "difficulty": 67957790298897.88,
    "time": 1701821874,
    "mediantime": 1701815216,
    "verificationprogress": 0.9999949712877075,
    "initialblockdownload": false,
    "chainwork": "00000000000000000000000000000000000000005fe0a5d6ff4ec8791944d4a0",
    "size_on_disk": 603215990915,
    "pruned": false,
    "warnings": ""
  },
  "error": null,
  "id": 1
}
```

</details>

### getblockcount

Returns the height of the most-work fully-validated chain.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getblockcount",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

```
{
  "result": 819942,
  "error": null,
  "id": 1
}
```

</details>

### getblockhash

Returns hash of block in best-block-chain at height provided.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getblockhash",
    "params": [
      819942
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**blockheight**: string (required) - The height index

</details>

<details>

<summary>Response</summary>

```
{
  "result": "0000000000000000000015d6cd7fe70e544f403df9c5c0b7504c2fe32a0a4e2c",
  "error": null,
  "id": 1
}
```

</details>

### getblockheader

If verbose is false, returns a string that is serialized, hex-encoded data for blockheader ‘hash’.

If verbose is true, returns an Object with information about blockheader ‘hash’.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getblockheader",
    "params": [
      "00000000c937983704a73af28acdec37b049d214adbda81d7e2a3dd146f6ed09",
      true
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**blockhash**: string (required)

**verbosity**: boolean (optional, default = true)

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "hash": "00000000c937983704a73af28acdec37b049d214adbda81d7e2a3dd146f6ed09",
    "confirmations": 818943,
    "height": 1000,
    "version": 1,
    "versionHex": "00000001",
    "merkleroot": "fe28050b93faea61fa88c4c630f0e1f0a1c24d0082dd0e10d369e13212128f33",
    "time": 1232346882,
    "mediantime": 1232344831,
    "nonce": 2595206198,
    "bits": "1d00ffff",
    "difficulty": 1,
    "chainwork": "000000000000000000000000000000000000000000000000000003e903e903e9",
    "nTx": 1,
    "previousblockhash": "0000000008e647742775a230787d66fdf92c46a48c896bfbc85cdc8acc67e87d",
    "nextblockhash": "00000000a2887344f8db859e372e7e4bc26b23b9de340f725afbf2edb265b4c6"
  },
  "error": null,
  "id": 1
}
```

</details>

### getblockstats

Returns block information.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getblockstats",
    "params": [
      "00000000c937983704a73af28acdec37b049d214adbda81d7e2a3dd146f6ed09",
      [
        "height",
        "time"
      ]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**blockhash | blockheight**: string (required) - The block hash or height of the target block

**stats**: json array (optional, default=all values) - Values to return

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "height": 1000,
    "time": 1232346882
  },
  "error": null,
  "id": 1
}
```

</details>

### getchaintips

Return information about all known tips in the block tree, including the main chain as well as orphaned branches.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getchaintips",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

**height**: (numeric)  - Height of the chain tip

**hash**:  (string) block hash of the tip

**branchlen**: (numeric) zero for main chain, otherwise length of branch connecting the tip to the main chain

**status**: (string) status of the chain, "active" for the main chain&#x20;

&#x20;    Possible values for status:

&#x20;         **invalid** - This branch contains at least one invalid block

&#x20;         **headers-only** - Not all blocks for this branch are available, but the headers are valid

&#x20;         **valid-headers** - All blocks are available for this branch, but they weren't fully validated

&#x20;         **valid-fork** - This branch is not part of the active chain, but is fully validated

&#x20;         **active** - This is the tip of the active main chain, which is certainly valid

```
{
  "result": [
    {
      "height": 819942,
      "hash": "0000000000000000000015d6cd7fe70e544f403df9c5c0b7504c2fe32a0a4e2c",
      "branchlen": 0,
      "status": "active"
    },
    {
      "height": 819343,
      "hash": "0000000000000000000262af3fb1dd3bc62ad5a119c48a57267a3b96145ce82c",
      "branchlen": 1,
      "status": "valid-fork"
    }
  ],
  "error": null,
  "id": 1
}
```

</details>

### getchaintxstats

Compute statistics about the total number and rate of transactions in the chain

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getchaintxstats",
    "params": [
      2016
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**nblocks**: number (optional, default = one month) - Size of the window in number of blocks

**blockhash**: string (optional, default = chain tip) - The hash of the block that ends the window.

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "time": 1701824262,
    "txcount": 931084234,
    "window_final_block_hash": "00000000000000000003d385edc272d96e39ce47f57c6b06ab793cf6f5bbd933",
    "window_final_block_height": 819943,
    "window_block_count": 2016,
    "window_tx_count": 7185472,
    "window_interval": 1172606,
    "txrate": 6.12778034565745
  },
  "error": null,
  "id": 1
}
```

</details>

### getdifficulty

Returns the proof-of-work difficulty as a multiple of the minimum difficulty.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getdifficulty",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

```
{
  "result": 67957790298897.88,
  "error": null,
  "id": 1
}
```

</details>

### getmempoolancestors

If txid is in the mempool, returns all in-mempool ancestors.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getmempoolancestors",
    "params": [
      "fadd2d64a82f96bb2cd7743106e7a69933c97175c51a7f0cab10103c53e0a7a6",
      true
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**txid**: string (required) - The transaction id (must be in mempool)

**verbosity**: boolean (optional, default = false) - True for a json object, false for array of transaction ids

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "c80f3d1e6245393f139cd2f9412e65a3ff0da2354a76760db43c9525a760cebc": {
      "vsize": 205,
      "weight": 817,
      "time": 1701821070,
      "height": 819940,
      "descendantcount": 4,
      "descendantsize": 1134,
      "ancestorcount": 1,
      "ancestorsize": 205,
      "wtxid": "dce49751f20450731fefe73d2240f96add003653da73eaecda81676a5e1a9e80",
      "fees": {
        "base": 0.0004961,
        "modified": 0.0004961,
        "ancestor": 0.0004961,
        "descendant": 0.0032486
      },
      "depends": [],
      "spentby": [
        "087898a3b486b9fc5ded7f76c8f2beff0683e00febf0d16e9f8cb8192eb0cfe5"
      ],
      "bip125-replaceable": false,
      "unbroadcast": false
    },
    "087898a3b486b9fc5ded7f76c8f2beff0683e00febf0d16e9f8cb8192eb0cfe5": {
      "vsize": 332,
      "weight": 1328,
      "time": 1701822665,
      "height": 819942,
      "descendantcount": 3,
      "descendantsize": 929,
      "ancestorcount": 2,
      "ancestorsize": 537,
      "wtxid": "9a88e7515a410f6c91b450e2613544c5c3489577ce06165546c74f5abe377c9d",
      "fees": {
        "base": 0.00096903,
        "modified": 0.00096903,
        "ancestor": 0.00146513,
        "descendant": 0.0027525
      },
      "depends": [
        "c80f3d1e6245393f139cd2f9412e65a3ff0da2354a76760db43c9525a760cebc"
      ],
      "spentby": [
        "40253b35c0cf5fd5987e86118c9accbb87bb7d8a7b1823006b9611c3ab6f4feb"
      ],
      "bip125-replaceable": false,
      "unbroadcast": false
    },
    "40253b35c0cf5fd5987e86118c9accbb87bb7d8a7b1823006b9611c3ab6f4feb": {
      "vsize": 267,
      "weight": 1065,
      "time": 1701824112,
      "height": 819942,
      "descendantcount": 2,
      "descendantsize": 597,
      "ancestorcount": 3,
      "ancestorsize": 804,
      "wtxid": "b4f08503b2a79c5dc9c767347c422fd45e284ac71ff23bf865617bcbbdb544db",
      "fees": {
        "base": 0.00077697,
        "modified": 0.00077697,
        "ancestor": 0.0022421,
        "descendant": 0.00178347
      },
      "depends": [
        "087898a3b486b9fc5ded7f76c8f2beff0683e00febf0d16e9f8cb8192eb0cfe5"
      ],
      "spentby": [
        "fadd2d64a82f96bb2cd7743106e7a69933c97175c51a7f0cab10103c53e0a7a6"
      ],
      "bip125-replaceable": false,
      "unbroadcast": false
    }
  },
  "error": null,
  "id": 1
}
```

</details>

### getmempooldescendants

If txid is in the mempool, returns all in-mempool descendants.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getmempooldescendants",
    "params": [
      "f1e2075eb37a38e368569eed8a2354f5289cfa9e5042b65606460f803644d8b6",
      true
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**txid**: string (required) - The transaction id (must be in mempool)

**verbosity**: boolean (optional, default = false) - True for a json object, false for array of transaction ids

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "794dc406d108c25d926c67a2488ab086a902620e9996b27667b1db751cc3ec09": {
      "vsize": 233,
      "weight": 930,
      "time": 1701770058,
      "height": 819869,
      "descendantcount": 3,
      "descendantsize": 699,
      "ancestorcount": 22,
      "ancestorsize": 6515,
      "wtxid": "b66b33cb9968a334ed03fb84133d2bdce5efd5158d27251e71a89766bbd280be",
      "fees": {
        "base": 5.76e-05,
        "modified": 5.76e-05,
        "ancestor": 0.002781,
        "descendant": 0.0001728
      },
      "depends": [
        "88ddf8945be48f77f1e10c6ae73b4d1ebcaefd8cb0c221a1006dee92d46e031c"
      ],
      "spentby": [
        "6a768dff5510be509ae86a89a051f21126aae2f0ed87344467dfa0e133f76557"
      ],
      "bip125-replaceable": true,
      "unbroadcast": false
    },
    "88ddf8945be48f77f1e10c6ae73b4d1ebcaefd8cb0c221a1006dee92d46e031c": {
      "vsize": 232,
      "weight": 928,
      "time": 1701770048,
      "height": 819869,
      "descendantcount": 4,
      "descendantsize": 931,
      "ancestorcount": 21,
      "ancestorsize": 6282,
      "wtxid": "feca93b727172b5cf5cb61e558afa654243ec3b6ad966981610bc2496991a9ae",
      "fees": {
        "base": 5.76e-05,
        "modified": 5.76e-05,
        "ancestor": 0.0027234,
        "descendant": 0.0002304
      },
      "depends": [
        "30b8a342c1ef839ca2a5c16f48530499fe3ba25367a599632931e0217d5f82a3"
      ],
      "spentby": [
        "794dc406d108c25d926c67a2488ab086a902620e9996b27667b1db751cc3ec09"
      ],
      "bip125-replaceable": true,
      "unbroadcast": false
    },
    "6a768dff5510be509ae86a89a051f21126aae2f0ed87344467dfa0e133f76557": {
      "vsize": 233,
      "weight": 930,
      "time": 1701770068,
      "height": 819869,
      "descendantcount": 2,
      "descendantsize": 466,
      "ancestorcount": 23,
      "ancestorsize": 6748,
      "wtxid": "a07de92cd73baca83100803bfacc8deddd6832fd4cd0db0d6476a1110584bd54",
      "fees": {
        "base": 5.76e-05,
        "modified": 5.76e-05,
        "ancestor": 0.0028386,
        "descendant": 0.0001152
      },
      "depends": [
        "794dc406d108c25d926c67a2488ab086a902620e9996b27667b1db751cc3ec09"
      ],
      "spentby": [
        "68d0adbcd7934c7c3216226164c2693f33018c24486ebadabcf20d26069f51f0"
      ],
      "bip125-replaceable": true,
      "unbroadcast": false
    },
    "30b8a342c1ef839ca2a5c16f48530499fe3ba25367a599632931e0217d5f82a3": {
      "vsize": 233,
      "weight": 930,
      "time": 1701770015,
      "height": 819869,
      "descendantcount": 5,
      "descendantsize": 1164,
      "ancestorcount": 20,
      "ancestorsize": 6050,
      "wtxid": "40a263fcd10a72bf753448822194d98bb14be41a5c04480310df8d4e389c9b52",
      "fees": {
        "base": 5.76e-05,
        "modified": 5.76e-05,
        "ancestor": 0.0026658,
        "descendant": 0.000288
      },
      "depends": [
        "dc44e30dd16a6b9e10f3b24d18dcf965a19cb2a76c3f92168770c5fc3798d480"
      ],
      "spentby": [
        "88ddf8945be48f77f1e10c6ae73b4d1ebcaefd8cb0c221a1006dee92d46e031c"
      ],
      "bip125-replaceable": true,
      "unbroadcast": false
    },
    "68d0adbcd7934c7c3216226164c2693f33018c24486ebadabcf20d26069f51f0": {
      "vsize": 233,
      "weight": 929,
      "time": 1701770085,
      "height": 819869,
      "descendantcount": 1,
      "descendantsize": 233,
      "ancestorcount": 24,
      "ancestorsize": 6981,
      "wtxid": "9c1359e6be44c1455da5e88de1aa55fcda040fcae7a223ff8c6efa76e151a9a5",
      "fees": {
        "base": 5.76e-05,
        "modified": 5.76e-05,
        "ancestor": 0.0028962,
        "descendant": 5.76e-05
      },
      "depends": [
        "6a768dff5510be509ae86a89a051f21126aae2f0ed87344467dfa0e133f76557"
      ],
      "spentby": [],
      "bip125-replaceable": true,
      "unbroadcast": false
    }
  },
  "error": null,
  "id": 1
}
```

</details>

### getmempoolentry

Returns mempool data for given transaction

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getmempoolentry",
    "params": [
      "00000000c937983704a73af28acdec37b049d214adbda81d7e2a3dd146f6ed09"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**txid**: string (required) - The transaction id (must be in mempool)

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "vsize": 233,
    "weight": 930,
    "time": 1701770004,
    "height": 819869,
    "descendantcount": 6,
    "descendantsize": 1397,
    "ancestorcount": 19,
    "ancestorsize": 5817,
    "wtxid": "8938f4c9d0805c70a40034c355eec1f84f5dde309858b604422e3aa235f5d2ff",
    "fees": {
      "base": 5.76e-05,
      "modified": 5.76e-05,
      "ancestor": 0.0026082,
      "descendant": 0.0003456
    },
    "depends": [
      "564a60e5143d34fe31e5d998141ce5636178795b7664f608b20b45087e517381"
    ],
    "spentby": [
      "30b8a342c1ef839ca2a5c16f48530499fe3ba25367a599632931e0217d5f82a3"
    ],
    "bip125-replaceable": true,
    "unbroadcast": false
  },
  "error": null,
  "id": 1
}
```

</details>

### getmempoolinfo

Returns details on the active state of the TX memory pool.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getmempoolinfo",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "loaded": true,
    "size": 132641,
    "bytes": 51269124,
    "usage": 295901136,
    "total_fee": 44.10890434,
    "maxmempool": 300000000,
    "mempoolminfee": 0.00020786,
    "minrelaytxfee": 1e-05,
    "incrementalrelayfee": 1e-05,
    "unbroadcastcount": 0,
    "fullrbf": false
  },
  "error": null,
  "id": 1
}
```

</details>

### getrawmempool

Returns all transaction ids in memory pool as a json array of string transaction ids.

_Hint: use `getmempoolentry` to fetch a specific transaction from the mempool._

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getrawmempool",
    "params": [
      false
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**verbosity**: boolean (optional, default = false) - True for a json object, false for array of transaction ids

**mempool\_sequence**: boolean (optional, default = false) - If verbose=false, returns a json object with transaction list and mempool sequence number attached.

</details>

<details>

<summary>Response</summary>

```
{
  "result": [
    "b9f9175094cfc470c06d07f6c932517d6231d8ac85517860e0a8602e0a189e82",
    "ab3f3baea4b4f5e365b299a2e3b69c042c99ee67399b991e13056a3c6bd8ae3d",
    "d1f138493d50c831b33144e69e4151d1960df30c54472eb728fe0bbb479bf893",
    "5bfee435b7f6bb2e46135f8dd0cf3881abef682019f5de82ffac995232909942",
    "333e091d4213a8d769b9139d7b5e4ad9523ca258cf998d6c739ce87870666de0",
    "1b5ec8ffdf1e653532af0fbd6d47453684496e87b89ff87e9f8fdd7c439640bf"
  ],
  "error": null,
  "id": 1
}
```

</details>

### gettxout

Returns details about an unspent transaction output.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_gettxout",
    "params": [
      "1b5ec8ffdf1e653532af0fbd6d47453684496e87b89ff87e9f8fdd7c439640bf",
      1
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - The transaction id

**n**: number (required) - vout number

**include\_mempool**: boolean (optional) - Whether to include the mempool. Note that an unspent output that is spent in the mempool won’t appear.

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "bestblock": "00000000000000000002b5d2570d68d505b52d0311ff42f13928d08ee4d1b7b5",
    "confirmations": 0,
    "value": 1.096e-05,
    "scriptPubKey": {
      "asm": "0 47c0996ec0694eeb4e6b2ec2a512a0d682bb29b6",
      "desc": "addr(bc1qglqfjmkqd98wknnt9mp22y4q66ptk2dkshz6xg)#6nv64a8x",
      "hex": "001447c0996ec0694eeb4e6b2ec2a512a0d682bb29b6",
      "address": "bc1qglqfjmkqd98wknnt9mp22y4q66ptk2dkshz6xg",
      "type": "witness_v0_keyhash"
    },
    "coinbase": false
  },
  "error": null,
  "id": 1
}
```

</details>

### gettxoutproof

Returns a hex-encoded proof that “txid” was included in a block.

NOTE: By default this function only works sometimes. This is when there is an unspent output in the utxo for this transaction. To make it always work, you need to maintain a transaction index, using the -txindex command line option or specify the block in which the transaction is included manually (by blockhash).

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_gettxoutproof",
    "params": [
      ["1b5ec8ffdf1e653532af0fbd6d47453684496e87b89ff87e9f8fdd7c439640bf"]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**txids**: string\[] (required) - The txids to filter

**blockhash**: string (optional) - If specified, looks for txid in the block with this hash

</details>

<details>

<summary>Response</summary>

```
{
  "result": "0040cc2498ac05b7d7af45e1ca32bc01ba0e2ac9a7445131b0fc03000000000000000000d7641e92f0b3e503ae6fbbefe8e965d93cd416df8f1db933be4b71cfb4c046fa71cd6f65502404170baa99b67a0a00000d057ff517528695fed2025deaf2c9f8816a5c009e46a9c9a1ec94a79eb4fcbe04e19b92d64607a82b10a017d36b9b3dd6a86dc942def84e159e7c832ec84ac414ef07a158a76e7b8c24b75029d548d8f2f109267611c139b9140801caf9cbd6b848c1b65b294687c90c094a21c5aa64e1a512319551d61c064bf73e0250c44e55e7778b7fb27f6a4689d9203bc3b22bd0df3b599cd299277771ff8b6f811b5bbb243ab61c1ba7bedc4cfb3104066300deb81b68d960e63e02520368da7b1c3d1c43ce73991525557ee80f0bf1b5fd9924dd8fc1dd738257dc6f97f886871f5a3ca8138bd270042ac50fa0c98f1c9900d26bf90a5172442fcd9138784107050ab10f66ef59bc9ecd907d6ade9bf5f863ba8f730c6da8cff199d5052466711fee4e24e0a1dfa9526b5c71eb42cb622d71edccba148e6eae784eae2dd7782547037d20edeaaa87de02e4e290e9b5becd2a58ebd4d75d83f554cf268c8527da9c312a479260ae7b26092425082464dbc56045f4478c074a84c1466d8eed65be420317e737753cb4433f2efaf2a75c450fdd49502d2a2fea07f4477ec7b295c38638c304db5a1500",
  "error": null,
  "id": 1
}
```

</details>

### gettxoutsetinfo

Returns statistics about the unspent transaction output set.

Note this call may take some time.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_gettxoutsetinfo",
    "params": []
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**hash\_type**: string (optional, default = hash\_serialized\_2) - Which UTXO set hash should be calculated. Options: ‘hash\_serialized\_2’ (the legacy algorithm), ‘none’.&#x20;

</details>

<details>

<summary>Response</summary>

```
{
  "result": {
    "height": 819947,
    "bestblock": "00000000000000000002201b4d518b345e7a036e38ec6575e551d2519396d0a3",
    "txouts": 140290697,
    "bogosize": 10639294518,
    "hash_serialized_2": "aaf099516fe9721b3258fc99a6bdc4c7b060105b1804ec4566498caf322c7d10",
    "total_amount": 19561955.53212622,
    "transactions": 97192652,
    "disk_size": 9212506394
  },
  "error": null,
  "id": 1
}
```

</details>

### verifytxoutproof

Verifies that a proof points to a transaction in a block, returning the transaction it commits to and throwing an RPC error if the block is not in our best chain

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_verifytxoutproof",
    "params": [
      "0040cc2498ac05b7d7af45e1ca32bc01ba0e2ac9a7445131b0fc03000000000000000000d7641e92f0b3e503ae6fbbefe8e965d93cd416df8f1db933be4b71cfb4c046fa71cd6f65502404170baa99b67a0a00000d057ff517528695fed2025deaf2c9f8816a5c009e46a9c9a1ec94a79eb4fcbe04e19b92d64607a82b10a017d36b9b3dd6a86dc942def84e159e7c832ec84ac414ef07a158a76e7b8c24b75029d548d8f2f109267611c139b9140801caf9cbd6b848c1b65b294687c90c094a21c5aa64e1a512319551d61c064bf73e0250c44e55e7778b7fb27f6a4689d9203bc3b22bd0df3b599cd299277771ff8b6f811b5bbb243ab61c1ba7bedc4cfb3104066300deb81b68d960e63e02520368da7b1c3d1c43ce73991525557ee80f0bf1b5fd9924dd8fc1dd738257dc6f97f886871f5a3ca8138bd270042ac50fa0c98f1c9900d26bf90a5172442fcd9138784107050ab10f66ef59bc9ecd907d6ade9bf5f863ba8f730c6da8cff199d5052466711fee4e24e0a1dfa9526b5c71eb42cb622d71edccba148e6eae784eae2dd7782547037d20edeaaa87de02e4e290e9b5becd2a58ebd4d75d83f554cf268c8527da9c312a479260ae7b26092425082464dbc56045f4478c074a84c1466d8eed65be420317e737753cb4433f2efaf2a75c450fdd49502d2a2fea07f4477ec7b295c38638c304db5a1500"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**proof**: string (required) - The hex-encoded proof generated by gettxoutproof

</details>

<details>

<summary>Response</summary>

```
{
  "result": [
    "4eee1f71662405d599f1cfa86d0c738fba63f8f59bde6a7d90cd9ebc59ef660f"
  ],
  "error": null,
  "id": 1
}
```

</details>

## Rawtransactions RPCs

### analyzepsbt

`btc_analyzepsbt` analyzes and provides information about the current status of a PSBT and its inputs.

{% tabs %}
{% tab title="curl" %}
{% code overflow="wrap" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_analyzepsbt",
    "params": [
"cHNidP8BAP0GAQIAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP////8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAA/////ww/Ybqlrf3R1mw1IuhEE/cL65NEjkYLabSBnsR2PuGpAAAAAAD/////AwAAAAAAAAAAIlEgoFjEyXO/4q/9a4OxcayNLtomxD4gYFOv5G3U3ehS0gEAAAAAAAAAACJRIKBYxMlzv+Kv/WuDsXGsjS7aJsQ+IGBTr+Rt1N3oUtIBOEoAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogAAAAAAAQErAAAAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogETQNpOi9nxzWlptSf4lamevUQFikJDYJuaS43lnWrUsPNB+5MNSyIWh+vkCYVz3KYOY1rEHDdGU+0Yi7/Ow7NBNiIBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgABASsAAAAAAAAAACJRIDxVHMDE9QTzP7Wv4FscqM17KAB2/iG8JrSa9QrGQ9qiARNAzBZ3R61BaOAoXq6jJg3nsDnwLjnqN6RKQjXCTDU/vUJ9hASFp9/iL+STGFNpM2PvMBmRWEYfPHlR+jb8tdoCwQEXIL1PX6SI6zdJBFA1kgWq5t7rpTzrEMI4xQ/hqjLuPOaGAAEBKyICAAAAAAAAIlEgPFUcwMT1BPM/ta/gWxyozXsoAHb+IbwmtJr1CsZD2qIBAwSDAAAAARNBi/O44iCmOLCTqXMcPJ6oamcxzVB8SDz25n8E5Pzc9LsjVEhfbL32r+/MBYtCouxLecbYXHeiQDQVtr4VwI8T0oMBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgAAAAA="
    ]
}'
```
{% endcode %}
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`psbt`**: string (required) - A base64 string of a PSBT.

</details>

<details>

<summary>Response</summary>

```json
{
    "result": {
        "inputs": [
            {
                "has_utxo": true,
                "is_final": false,
                "next": "finalizer"
            },
            {
                "has_utxo": true,
                "is_final": false,
                "next": "finalizer"
            },
            {
                "has_utxo": true,
                "is_final": false,
                "next": "finalizer"
            }
        ],
        "fee": -0.00018454,
        "next": "finalizer"
    },
    "error": null,
    "id": 1
}
```

</details>

### combinepsbt

`btc_combinepsbt` combines multiple partially signed PSBTs into a single PSBT. It takes partially signed transactions from various sources and consolidates them. This is important in multi-signature or complex transaction processes where each party contributes part of the transaction's data or signatures.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_combinepsbt",
    "params": [
      [
        "cHNidP8BAP0GAQIAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP////8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAA/////ww/Ybqlrf3R1mw1IuhEE/cL65NEjkYLabSBnsR2PuGpAAAAAAD/////AwAAAAAAAAAAIlEgoFjEyXO/4q/9a4OxcayNLtomxD4gYFOv5G3U3ehS0gEAAAAAAAAAACJRIKBYxMlzv+Kv/WuDsXGsjS7aJsQ+IGBTr+Rt1N3oUtIBOEoAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogAAAAAAAQErAAAAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogETQNpOi9nxzWlptSf4lamevUQFikJDYJuaS43lnWrUsPNB+5MNSyIWh+vkCYVz3KYOY1rEHDdGU+0Yi7/Ow7NBNiIBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgABASsAAAAAAAAAACJRIDxVHMDE9QTzP7Wv4FscqM17KAB2/iG8JrSa9QrGQ9qiARNAzBZ3R61BaOAoXq6jJg3nsDnwLjnqN6RKQjXCTDU/vUJ9hASFp9/iL+STGFNpM2PvMBmRWEYfPHlR+jb8tdoCwQEXIL1PX6SI6zdJBFA1kgWq5t7rpTzrEMI4xQ/hqjLuPOaGAAEBKyICAAAAAAAAIlEgPFUcwMT1BPM/ta/gWxyozXsoAHb+IbwmtJr1CsZD2qIBAwSDAAAAARNBi/O44iCmOLCTqXMcPJ6oamcxzVB8SDz25n8E5Pzc9LsjVEhfbL32r+/MBYtCouxLecbYXHeiQDQVtr4VwI8T0oMBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgAAAAA=",
        "cHNidP8BAP0GAQIAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP////8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAA/////ww/Ybqlrf3R1mw1IuhEE/cL65NEjkYLabSBnsR2PuGpAAAAAAD/////AwAAAAAAAAAAIlEgoFjEyXO/4q/9a4OxcayNLtomxD4gYFOv5G3U3ehS0gEAAAAAAAAAACJRIKBYxMlzv+Kv/WuDsXGsjS7aJsQ+IGBTr+Rt1N3oUtIBOEoAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogAAAAAAAQErAAAAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogETQNpOi9nxzWlptSf4lamevUQFikJDYJuaS43lnWrUsPNB+5MNSyIWh+vkCYVz3KYOY1rEHDdGU+0Yi7/Ow7NBNiIBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgABASsAAAAAAAAAACJRIDxVHMDE9QTzP7Wv4FscqM17KAB2/iG8JrSa9QrGQ9qiARNAzBZ3R61BaOAoXq6jJg3nsDnwLjnqN6RKQjXCTDU/vUJ9hASFp9/iL+STGFNpM2PvMBmRWEYfPHlR+jb8tdoCwQEXIL1PX6SI6zdJBFA1kgWq5t7rpTzrEMI4xQ/hqjLuPOaGAAEBKyICAAAAAAAAIlEgPFUcwMT1BPM/ta/gWxyozXsoAHb+IbwmtJr1CsZD2qIBAwSDAAAAARNBi/O44iCmOLCTqXMcPJ6oamcxzVB8SDz25n8E5Pzc9LsjVEhfbL32r+/MBYtCouxLecbYXHeiQDQVtr4VwI8T0oMBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgAAAAA="
      ]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`psbts`**: \[string] (required) - An array of base64-encoded PSBTs. These are the PSBTs that are to be combined. It's assumed that each PSBT in the array contains some of the necessary data or signatures needed for the final transaction.

</details>

<details>

<summary>Response</summary>

{% code overflow="wrap" %}
```json
{
    "result": "cHNidP8BAP0GAQIAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP////8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAA/////ww/Ybqlrf3R1mw1IuhEE/cL65NEjkYLabSBnsR2PuGpAAAAAAD/////AwAAAAAAAAAAIlEgoFjEyXO/4q/9a4OxcayNLtomxD4gYFOv5G3U3ehS0gEAAAAAAAAAACJRIKBYxMlzv+Kv/WuDsXGsjS7aJsQ+IGBTr+Rt1N3oUtIBOEoAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogAAAAAAAQErAAAAAAAAAAAiUSA8VRzAxPUE8z+1r+BbHKjNeygAdv4hvCa0mvUKxkPaogETQNpOi9nxzWlptSf4lamevUQFikJDYJuaS43lnWrUsPNB+5MNSyIWh+vkCYVz3KYOY1rEHDdGU+0Yi7/Ow7NBNiIBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgABASsAAAAAAAAAACJRIDxVHMDE9QTzP7Wv4FscqM17KAB2/iG8JrSa9QrGQ9qiARNAzBZ3R61BaOAoXq6jJg3nsDnwLjnqN6RKQjXCTDU/vUJ9hASFp9/iL+STGFNpM2PvMBmRWEYfPHlR+jb8tdoCwQEXIL1PX6SI6zdJBFA1kgWq5t7rpTzrEMI4xQ/hqjLuPOaGAAEBKyICAAAAAAAAIlEgPFUcwMT1BPM/ta/gWxyozXsoAHb+IbwmtJr1CsZD2qIBAwSDAAAAARNBi/O44iCmOLCTqXMcPJ6oamcxzVB8SDz25n8E5Pzc9LsjVEhfbL32r+/MBYtCouxLecbYXHeiQDQVtr4VwI8T0oMBFyC9T1+kiOs3SQRQNZIFqube66U86xDCOMUP4aoy7jzmhgAAAAA=",
    "error": null,
    "id": 1
}
```
{% endcode %}

</details>

### combinerawtransaction

`btc_combinerawtransaction` combines multiple partially signed raw transactions, in hexidecimal format, into one transaction. The combined transaction may be another partially signed transaction or a fully signed transaction. Each input transaction must have the same output structure to be successfully combined.

Useful for multi-signature transactions where each party signs the transaction separately. `btc_combinerawtransaction` collects these partial signatures and combines them into one transaction.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_combinerawtransaction",
    "params": [
      [
        "<txs>",
      ]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txs`**: \[string] (required) - An array of raw transaction strings (in hexadecimal format). These are the partially signed or unsigned transactions that you want to combine.

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### converttopsbt

`btc_converttopsbt` converts a standard Bitcoin raw transaction into a PSBT. This functionality is particularly useful when starting with a non-PSBT transaction format and needing to transition to a PSBT for multi-signature or advanced signing workflows. The conversion process ensures that the resulting PSBT remains unsigned, making it suitable for passing around to various parties for signatures.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_converttopsbt",
    "params": [
      "<raw_txn>",
      true
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`raw_txn`**: string (required) - The hexadecimal string of the raw transaction that you wish to convert into a PSBT. This is the serialized, hex-encoded form of a standard Bitcoin transaction.

**`permitsigdata`**: boolean (optional) - A boolean parameter. If set to `true`, it allows the inclusion of signature scripts and witness data in the PSBT if they are present in the input transaction. This can be useful if the raw transaction already contains some signatures, but generally, this should be `false` to keep the PSBT unsigned.

**`iswitness`**: boolean (optional) - A boolean parameter indicating whether the transaction is a witness transaction. Setting this to `true` is necessary for Segregated Witness (SegWit) transactions to ensure the correct construction of the PSBT.

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### createpsbt

`btc_createpsbt` creates a PSBT. It can be used for creating transactions that require multiple signatures or specific signing arrangements.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_createpsbt",
    "params": [
      "640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59",
      true
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`inputs`**: string array (required) - An array of JSON objects, each representing an input to the transaction. Each object typically contains the following fields:

* `txid`: The transaction ID from which you're spending an output.
* `vout`: The index of the output in the previous transaction (starting from `0`).
* `sequence` (optional): The sequence number of the input. It can be used for more advanced features like Replace-By-Fee (RBF).

**`outputs`**: string (required) - This can be either a JSON object or an array of JSON objects specifying the transaction's outputs. There are two formats to provide this information:

* **Object format**: Where the object's keys are Bitcoin addresses or a scriptPubKey, and values are amounts of bitcoin to send.
* **Array format**: Each element of the array is an object with `address` or `scriptPubKey` and `amount`.

**`locktime`**: string (optional) - A Unix timestamp or block number that determines when the transaction may be added to a block. A transaction with a locktime set won't be included in a block before the specified time or block number.

**`replaceable`**: boolean (optional) - A boolean flag indicating whether the transaction should be marked as BIP125 replaceable, allowing it to be replaced by a transaction with a higher fee (if it's not yet confirmed).

**`options`**: string (optional) -  A JSON object where you can specify additional options such as `add_inputs` (to automatically add inputs to meet the output value requirements) and `change_address` (to set a specific change address).

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### createrawtransaction

`btc_createrawtransaction` creates a new, raw, and unsigned Bitcoin transaction.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_createrawtransaction",
    "params": [
      [
        "<inputs>"
      ],
      "<outputs>"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`inputs`**: string array (required) - An array of JSON objects, each representing an input to the transaction. Each object typically contains the following fields:

* `txid`: The transaction ID from which you're spending an output.
* `vout`: The index of the output in the previous transaction (starting from `0`).
* `sequence` (optional): The sequence number of the input. It can be used for more advanced features like Replace-By-Fee (RBF).

**`outputs`**: string (required) - Either an array of JSON objects or a single JSON object. Each output specifies an address and the amount of bitcoin to send to that address. There are two formats:

* **Object format**: Keys are Bitcoin addresses and values are amounts to send to those addresses.
* **Array format**: Each element is an object with `address` and `amount` fields.

**`locktime`**: string (optional) - A Unix timestamp or block number until which the transaction will be locked and not included in a block. If omitted, defaults to `0`, meaning no locktime.

**`replaceable`**: boolean (optional) - A boolean flag indicating whether the transaction should be marked as BIP125 replaceable, allowing it to be replaced by a transaction with a higher fee (if it's not yet confirmed).

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### decodepsbt

`btc_decodepsbt` decodes a base64-encoded PSBT into a structured, readable format. The decoded output provides comprehensive information about the PSBT, including its inputs, outputs, any signatures it contains, and additional metadata.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_decodepsbt",
    "params": [
      "<psbt>"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`psbt`**: string (required) - The base64-encoded PSBT that you want to decode.

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### decoderawtransaction

`btc_decoderawtransaction` decodes a serialized, hex-encoded transaction into a human-readable format.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_decoderawtransaction",
    "params": [
      "<hexstring>",
      false
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`hexstring`**: string (required) - The hex-encoded transaction data that you want to decode.

**`iswitness`**: boolean (optional, default = false) - Indicates whether the transaction includes SegWit data. If set to `true`, the decoder will expect and properly handle a Segregated Witness (SegWit) transaction format. If omitted or set to `false`, the decoder treats the transaction as a non-SegWit transaction.

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### decodescript

`btc_decodescript` is used to decode a hex-encoded script and provide human-readable details about it. The decoded information includes details such as the type of script (e.g., `P2PKH`, `P2SH`), required signatures, and any embedded addresses.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_decodescript",
    "params": [
      "52205af0d72e5a051ba2b5615136ee598dcae09aaf7f2dfbd51170ac18d7aae917968e8585e86048f2824278afa7abaa0527bc46314533d57a09c62216da38a5"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`scriptHex`**: string (required) - The hexadecimal encoded script that you want to decode. The script could be a transaction output script (scriptPubKey), a redeem script, or any other valid script in hex format.

</details>

<details>

<summary>Response</summary>

```json
{
    "result": {
        "asm": "2 5af0d72e5a051ba2b5615136ee598dcae09aaf7f2dfbd51170ac18d7aae91796 OP_2DIV OP_OR OP_OR OP_UNKNOWN 16 [error]",
        "desc": "raw(52205af0d72e5a051ba2b5615136ee598dcae09aaf7f2dfbd51170ac18d7aae917968e8585e86048f2824278afa7abaa0527bc46314533d57a09c62216da38a5)#3yep52eq",
        "type": "nonstandard"
    },
    "error": null,
    "id": 1
}
```

</details>

### finalizepsbt

`btc_finalizepsbt` is used to finalize a Partially Signed Bitcoin Transaction (PSBT). This process involves completing the signing of the transaction and converting it into a fully signed transaction that is ready to be broadcast to the Bitcoin network.&#x20;

Here's a breakdown of its functionality:

1. **Completing Signatures**: The primary function of `btc_finalizepsbt` is to complete the signing of all inputs in the PSBT. It checks if all necessary signatures are present and valid for each input.
2. **Constructing the Final Transaction**: Once all inputs are fully signed, `btc_finalizepsbt` constructs a final, standard Bitcoin transaction from the PSBT. This transaction is in a standard serialized transaction format, which is recognized and can be processed by the Bitcoin network.
3. **Ensuring Transaction Validity**: The API call also checks that the transaction is valid. This includes ensuring that all inputs are correctly signed and that the transaction conforms to Bitcoin's transaction rules.
4. **Extracting the Transaction**: If the PSBT is fully signed, `btc_finalizepsbt` extracts and returns the fully signed transaction in a hexadecimal string format. This string can then be directly submitted to the Bitcoin network for confirmation and inclusion in a block.
5. **Use in Multi-Party Transactions**: `btc_finalizepsbt` is particularly useful in multi-party transaction scenarios where a PSBT has been passed around multiple parties for signing. It provides a simple and secure way to finalize the transaction once all parties have provided their signatures.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_finalizepsbt",
    "params": [
      "<psbt>",
      true
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`psbt`**: string (required) - the PSBT in base64-encoded format.

**`extract`**: boolean (optional, default = true) - When set to `true`, the call will automatically extract and return the fully signed transaction in raw format (hexadecimal string) if the PSBT is finalized successfully. If set to `false`, the PSBT is finalized but the raw transaction is not automatically extracted.

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### getrawtransaction

`btc_getrawtransaction` retrieves detailed information about a specific transaction in the Bitcoin network. It's particularly useful for examining the details of transactions that are either in the mempool or already included in a block.

By default this function only works for mempool transactions. When called with a `blockhash` argument, `btc_getrawtransaction` will return the transaction if the specified block is available and the transaction is found in that block.&#x20;

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getrawtransaction",
    "params": [
      "9d92bea825ec5736f7cb0bb8b952b6bb0b7dc6f13f300245b2aece25acb31ef8",
      false
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - The transaction ID of the transaction you want to retrieve.

**`verbose`**: boolean (optional, default = false) - A boolean flag that indicates the format of the output. If set to `true` (or `1`), the output will be JSON formatted and contain detailed, human-readable information about the transaction. If set to `false` (or `0`), the output will be the raw transaction in hexadecimal format.

**`blockhash`**: string (optional) - The hash of the block in which to look for the transaction. If provided, it speeds up the search by limiting it to a specific block. It's particularly useful for transactions that are not in the mempool but included in a block.

</details>

<details>

<summary>Response</summary>

```json
{
    "result": "02000000000101196f48ccd7fbe671bd7cc0ec8a493ea75deb8ad8935ef9a9412f9e2c8a7f3ffe0500000000ffffffff02b036000000000000225120e41028ad8442a01e3651bb7806bbaa01ffef410ab6e08ce197a1c80169cbae9dc2672d00000000001600142bfe42a69e37751a96776c8151391c055ca1b460024730440220569dabe17b509c618a085be0270e451652e542242f8e6f8bd940f6d558500dc202206b11de91a7a465d1bb46c42d1df04cc223a05a758517f063e6336d381ec6b19f0121026de43f1d41eeba32b31ad449edb28ef0390b97828d7c76f61cbcc8cc4de6412300000000",
    "error": null,
    "id": 1
}
```

</details>

### joinpsbts

`btc_joinpsbts` is used for joining multiple Partially Signed Bitcoin Transactions (PSBTs) into a single PSBT. PSBT is a standard format for Bitcoin transactions that are not fully signed, allowing for transactions to be passed around multiple parties for signing without revealing the private keys.

Functionality of `btc_joinpsbts` includes:

1. **Combining Transactions**: It takes multiple PSBTs as input and combines them into one. This is particularly useful in scenarios where different parties have created separate PSBTs for different inputs or outputs, and these need to be consolidated into a single transaction.
2. **Facilitating Multi-Party Transactions**: In situations where a transaction requires inputs or consents from multiple parties, each party can create their own PSBT. `btc_joinpsbts` then merges these into a single PSBT that encompasses all the inputs, outputs, and signatures provided by the individual parties.
3. **Streamlining the Signing Process**: Once the PSBTs are joined, the resulting single PSBT can be passed to each relevant party for signing. This streamlines the process, as each party can sign the same transaction instead of having to deal with multiple transactions.
4. **Use in Complex Transaction Scenarios**: This call is particularly useful in complex transaction scenarios like multi-signature transactions, CoinJoin transactions, or batch processing of transactions.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_joinpsbts",
    "params": [
      [
        "<psbt>",
        "<psbt>"
      ]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`psbt_array`**: \[strings] (required) - An array of PSBTs in base64 string format. Each PSBT in the list represents a transaction that has been partially signed or constructed but not finalized.

</details>

<details>

<summary>Response</summary>

```json
```

</details>

### sendrawtransaction

Submit a raw transaction (serialized, hex-encoded) to local node and network.

Note that the transaction will be sent unconditionally to all peers, so using this for manual rebroadcast may degrade privacy by leaking the transaction’s origin, as nodes will normally not rebroadcast non-wallet transactions already in their mempool.

Also see createrawtransaction and signrawtransactionwithkey calls.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_sendrawtransaction",
    "params": [
      "<raw_tx>",
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`raw_tx`**: string (required) - The hex string of the raw transaction.

**`maxfeerate`**: number (number or string, optional, default=0.10) - Reject transactions whose fee rate is higher than the specified value, expressed in BTC/kB. Set to 0 to accept any fee rate.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### testmempoolaccept

`btc_testmempoolaccept` is used to test how a transaction would be received by the mempool without actually broadcasting it to the network. Here's an overview of its functionality:

1. **Transaction Validation**: It validates a raw transaction to check whether it would be accepted into the mempool. This validation includes various checks like transaction syntax, structure, and whether it meets the network's consensus rules.
2. **Fee and Size Checks**: The call assesses whether the transaction fees are adequate and if the transaction size is within acceptable limits.
3. **Double-Spend Checks**: It verifies that the transaction doesn't attempt to double-spend any UTXOs (Unspent Transaction Outputs).
4. **Chain State Consideration**: The API call considers the current state of the blockchain and the mempool to evaluate if the transaction would be accepted at that particular moment.
5. **No Actual Broadcast**: Importantly, `btc_testmempoolaccept` does not broadcast the transaction to the network. It's a purely local test to see if the transaction would be accepted by the mempool under current conditions.
6. **Response Data**: It returns information on whether the transaction would be accepted and, if not, provides details about why it would be rejected.

Note that the transaction will be sent unconditionally to all peers, so using this for manual rebroadcast may degrade privacy by leaking the transaction’s origin, as nodes will normalin getgly not rebroadcast non-wallet transactions already in their mempool.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_testmempoolaccept",
    "params": [
      [
        "<raw_txs>",
        "0.10"
        ]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`raw_txs`**: \[string] (required) - An array of raw transaction strings (in hexadecimal format). Each string represents a serialized transaction that you want to test for mempool acceptance. Even if you're testing just one transaction, it needs to be wrapped in an array.

**`maxfeerate`**: number (number or string, optional, default=0.10) - specifies the maximum fee rate, in satoshis per virtual byte, that a transaction can have to be considered for mempool acceptance in this test. If a transaction's fee rate is higher than this value, the `testmempoolaccept` call will reject it. If this parameter is not set, the node's default mempool maximum fee rate is used. Set to 0 to accept any fee rate.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### utxoupdatepsbt

`btc_utxoupdatepsbt` updates a PSBT with information about the UTXOs it spends. This is particularly useful in scenarios where a PSBT lacks certain details about the UTXOs, which are necessary for signing and finalizing the transaction.&#x20;

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_utxoupdatepsbt",
    "params": [
      "<psbt>"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`psbt`**: string (required) - The base64-encoded PSBT that needs updating. This PSBT should have inputs and outputs already defined but might be missing UTXO details.

**`descriptors`**: string array (optional) - An array of output descriptors that provide information about the output scripts of the UTXOs being spent. This parameter is optional but can be helpful in cases where the PSBT does not have full UTXO information.

</details>

<details>

<summary>Response</summary>

```
```

</details>



## Util RPCs

### createmultisig

`btc_createmultisig` creates a multi-signature address. Multi-signature addresses require more than one private key to authorize a transaction, enhancing the security of Bitcoin transactions

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_createmultisig",
    "params": [
      "2",
      [
        "<pubkey>",
        "<pubkey>"
      ]
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`nrequired`**: string (required) - An integer specifying the number of required signatures to spend bitcoins from the multi-signature address. For example, in a "2-of-3" multi-signature setup, `nrequired` would be `2`.

**`keys`**: string array (required) - An array of strings, where each string is a public key. These public keys are used to construct the multi-signature address. The number of keys in the array can be more than `nrequired`.

**`address_type`**: string (optional) - A string specifying the type of address to create. The options usually include `"legacy"` (for P2SH addresses) and `"bech32"` (for P2WSH addresses). If omitted, the default is typically `"legacy"`.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### deriveaddresses

`btc_deriveaddresses` derives one or more addresses from an output descriptor. Output descriptors provide a human-readable way to describe Bitcoin addresses, including information about how to spend coins sent to these addresses.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_deriveaddresses",
    "params": [
      "<descriptor>"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`descriptor`**: string (required) - A string representing the output descriptor from which addresses are to be derived. This descriptor includes details about the type of address, the public keys involved, and any additional scripting requirements.

**`begin`**: string (optional) - An integer specifying the starting index for address derivation. This is relevant for descriptors that define a range of addresses, like those used in HD wallets. If omitted, the default value is typically `0`.&#x20;

**`end`**: string (optional) - An integer specifying the ending index for address derivation. Like the `begin` parameter, this defines the range of addresses to derive from the descriptor. If omitted, the API call will only derive the address at the `begin` index.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### estimatesmartfee

`btc_estimatesmartfee` estimates the transaction fee per kilobyte that should be included with a transaction in order to have it confirmed within a certain number of blocks. This estimation is based on the current state of the mempool and recent blocks.&#x20;

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_estimatesmartfee",
    "params": [
      "6",
      "ECONOMICAL"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`conf_target`**: string (required) - An integer specifying the desired confirmation time, in number of blocks, for the transaction. For example, setting this to `6` would mean you want the transaction to be confirmed within approximately 6 blocks.

**`estimate_mode`**: string (optional) - A string indicating the estimation strategy to use. Possible values include:

* `"UNSET"` (default): The Bitcoin Core algorithm will select the best estimation mode automatically.
* `"ECONOMICAL"`: Provides fee estimates that aim to minimize costs.
* `"CONSERVATIVE"`: Provides higher fee estimates that prioritize confirmation reliability over cost.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### getdescriptorinfo

`btc_getdescriptorinfo` provides information about a descriptor. A descriptor is a more expressive way to describe Bitcoin script types for wallets, including information on how to spend outputs sent to these addresses. The call calculates and returns a checksum for the descriptor, which is used to verify its integrity.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getdescriptorinfo",
    "params": [
      "<descriptor>"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`descriptor`**: string (required) - A string representing the output descriptor to be analyzed. This descriptor should contain information about the script or address type, along with key information.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### getindexinfo

`btc_getindexinfo` obtains information about the state of the blockchain indexes. These indexes are additional structures maintained by the node, which provide faster access to certain types of data, like transaction data or block information.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getindexinfo",
    "params": [ ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

None

</details>

<details>

<summary>Response</summary>

```
```

</details>

### validateaddress

`btc_validateaddress` validates and provides detailed information about a Bitcoin address. If the address is valid, it provides detailed information about it, including the address type, whether it belongs to the wallet, and its associated public key (if available). For script addresses, it provides information about the script.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_validateaddress",
    "params": [
      "<address>"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`address`**: string (required) - The Bitcoin address to be validated.

</details>

<details>

<summary>Response</summary>

```
```

</details>

### verifymessage

`btc_verifymessage` verifies a signed message. This function is used in scenarios where you need to prove ownership of a Bitcoin address or validate the authenticity of a message signed with a Bitcoin private key.

```
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_verifymessage",
    "params": [
      "<address>",
      "<signature>",
      "<message>"
    ]
}'
```

<details>

<summary>Params</summary>

**`address`**: string (required) - The Bitcoin address used to sign the message.

**`signature`**: string (required) - The signature created by signing the message with the private key of the Bitcoin address.

**`message`**: string (required) - The original message that was signed.

</details>
