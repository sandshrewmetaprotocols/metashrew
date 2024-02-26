# Ordinals RPC

## Overview

The Sandshrew Ordinals Indexer is an advanced tool designed to explore and interact with Bitcoin-native digital artifacts, known as inscriptions. These unique entities, embedded within individual satoshis (sats), allow for the creation and transfer of distinct digital content directly on the Bitcoin blockchain. These digital artifacts can be securely held in a Bitcoin wallet and transferred seamlessly through Bitcoin transactions, just like any other Bitcoin asset.

Understanding and managing these inscriptions, however, goes beyond the capabilities of a standard Bitcoin setup. Accessing and interacting with this layer of the blockchain requires a full node to provide a real-time view of the blockchain's state and a specialized wallet capable of crafting and managing these unique inscriptions.

While Bitcoin Core offers the foundational elements — a full node and a wallet — it falls short in handling inscriptions. It neither supports the creation of these specialized entities nor offers the necessary sat control to manage their transfer. The Sandshrew Ordinals RPC Service bridges this gap, offering comprehensive tools for inscription creation, management, and transfer. Our service extends the functionality of a standard Bitcoin wallet, empowering users to engage with the world of Bitcoin ordinals effectively.&#x20;

### Inscription IDs

Ordinals inscriptions are contained within the inputs of a reveal transaction. In order to uniquely identify them they are assigned an ID of the form:

`521f8eccffa4c41a3a7728dd012ea5a4a02feed81f41159231251ecf1e5c79dai0`

The part in front of the `i` is the transaction ID (`txid`) of the reveal transaction. The number after the `i` defines the index (starting at 0) of new inscriptions being inscribed in the reveal transaction.

Inscriptions can either be located in different inputs, within the same input or a combination of both. In any case the ordering is clear, since a parser would go through the inputs consecutively and look for all inscription `envelopes`.&#x20;

([Reference](https://docs.ordinals.com/inscriptions.html#inscription-ids))

| Input | Inscription Count | Indices    |
| ----- | ----------------- | ---------- |
| 0     | 2                 | i0, i1     |
| 1     | 1                 | i2         |
| 2     | 3                 | i3, i4, i5 |
| 3     | 0                 |            |
| 4     | 1                 | i6         |

## Inscriptions

### Get Inscriptions

`ord_inscriptions` returns a specified number of inscription IDs, ordered by their `inscription number`. If no parameters are passed in, it returns the latest 100 inscriptions.

An inscription number is a unique identifier assigned to each individual inscription, serving as a sequential reference point within the Bitcoin blockchain's ordinals space. The `insc_num` parameter represents the starting inscription number from which the query will begin fetching inscriptions.&#x20;

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_inscriptions",
    "params": [
      "802285",
      "10"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`insc_num`**: string (optional) - Sets the starting point for the query based on the inscription number, a unique sequential identifier for each inscription within the Bitcoin blockchain's ordinals.

**`n`**: string (required) - Determines the number of inscriptions to return from the starting point.

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "inscriptions": [
      "ad6da68f83fe92a0eb9dac1b681a596386072cde173ff9dc846fc8408d9ac459i0",
      "3de665a880bffaa0364f910a29a35b6b28f1e73e17bc1226c647faf57b6e114fi0",
      "696adfa8f35a148cf826888d856b73de2b66254f048a667365fe3e2fd06fe847i0",
      "0123964d9fe95a5e01a9f361fb74f5a5b61045a9157ed3f29a149c9f3156a81ci0",
      "7cb048cd70873917b6b6791f369b842d150448c52c2fad008f52928a44f19502i0",
      "782e461824dff8ed97bd0454a35dcefa5089e22d0873bb55c4f08851e6a13990i0",
      "31f778f34d1da76a778b92fd404778882f000cb9d1b96afce38e22e81ca6f5ffi0",
      ...
    ],
    "prev": 46660491,
    "next": null,
    "lowest": 0,
    "highest": 46660591
  }
}
```

</details>

### Inscription by Block Hash

`ord_block` retrieves all inscriptions associated with a specific block hash.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_block",
    "params": [
      "000000000000000000063a92390ee25a1f0b41ccaf4e675227acd864dc2eb3dd"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - The hash of the block for which inscriptions are to be returned.

</details>

<details>

<summary>Response</summary>

<pre><code>{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "hash": "000000000000000000046431775aab38aee784155ab30b2c592ec221617c7a52",
    "target": "0000000000000000000676810000000000000000000000000000000000000000",
    "best_height": 820065,
    "height": 780236,
    "inscriptions": [
      "c8eabc7a52044d5bd597d38566f5952ab3f18c57f6fe4978f1916f4f359e694ai0",
      "7691b42dc310bc71757684b04dd70c848724ee56c9a4655ef6be93af2bcf2003i0",
      "5bd289781c7c8d9ccef7cc77a9c77c2a1002cf2124bebd46fbe1e1eb8b70120di0",
      "779244caab97ff4f480a0545bbddcbac935af4e461d1889c05176ee1ea94251fi0",
      "711663cfb7edf890b18fdf00c188acddd8eb248d92036a7545ff3fb51cd1fe40i0",
      "9fd0e3556f39f70442dde8fef5bafa527529d0d511efd9145b8638d6ffb1554fi0",
      "f58036830ce913a1bd414b430061cfe25d913c8f3db08ccc4223ce6e06fcde60i0"
<strong>    ]
</strong>  }
}    
</code></pre>

</details>

### Inscription by Block Height

`ord_inscriptions:block` fetches all inscriptions associated with a block at a specified block height. This endpoint supports pagination.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_inscriptions:block",
    "params": [
      "780236"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_height`**: string (required) - Specifies the height of the block from which inscriptions are to be retrieved.

**`page`**: string (optional) - Determines the page number of results, enabling paged viewing of inscriptions.

</details>

<details>

<summary>Response</summary>

<pre><code>{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "hash": "000000000000000000046431775aab38aee784155ab30b2c592ec221617c7a52",
    "target": "0000000000000000000676810000000000000000000000000000000000000000",
    "best_height": 820065,
    "height": 780236,
    "inscriptions": [
      "c8eabc7a52044d5bd597d38566f5952ab3f18c57f6fe4978f1916f4f359e694ai0",
      "7691b42dc310bc71757684b04dd70c848724ee56c9a4655ef6be93af2bcf2003i0",
      "5bd289781c7c8d9ccef7cc77a9c77c2a1002cf2124bebd46fbe1e1eb8b70120di0",
      "779244caab97ff4f480a0545bbddcbac935af4e461d1889c05176ee1ea94251fi0",
      "711663cfb7edf890b18fdf00c188acddd8eb248d92036a7545ff3fb51cd1fe40i0",
      "9fd0e3556f39f70442dde8fef5bafa527529d0d511efd9145b8638d6ffb1554fi0",
      "f58036830ce913a1bd414b430061cfe25d913c8f3db08ccc4223ce6e06fcde60i0"
<strong>    ]
</strong>  }
}    
</code></pre>

</details>

### Inscription by Tx Output

Returns inscription and Sat information from a transaction output

{% tabs %}
{% tab title="curl" %}
```sh
oucurl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_output",
    "params": [
      "22a0d4ad3fafb1eb53823b7655103bb7d6d7b61e9ac572e2f493bbdb8a371a09:0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid:vout`**: string (required) - Output transaction ID : The number of the transaction output

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "value": 6348,
    "script_pubkey": "OP_PUSHNUM_1 OP_PUSHBYTES_32 6d0247f995a3b2c5fbfa6594d1d23af277c5260d4c827b481c89206948ca332c",
    "address": "bc1pd5py07v45wevt7l6vk2dr5367fmu2fsdfjp8kjqu3ysxjjx2xvkquf5e7m",
    "transaction": "22a0d4ad3fafb1eb53823b7655103bb7d6d7b61e9ac572e2f493bbdb8a371a09",
    "sat_ranges": [
      [
        1596764664144241,
        1596764664150589
      ]
    ],
    "inscriptions": [
      "d1e0e792195d9da7be7ce46ec81fe31f7942e350a5da2baf4a7cffe91adb3fa1i0"
    ],
    "runes": {}
  }
}
```

</details>

### Inscription by ID

Returns inscription information based on an inscription ID.

{% tabs %}
{% tab title="curl" %}
```sh
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
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
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`inscription_id`**: string (required) - The inscription ID

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "address": "bc1p3d8rcefkztkvq9x54dhyh8d70n7wpxtm3z5lrrzacv70v8z39t2qd9396s",
        "children": [],
        "content_length": 190,
        "content_type": "image/png",
        "genesis_fee": 21513,
        "genesis_height": 822000,
        "inscription_id": "640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59i0",
        "inscription_number": 49497279,
        "next": "eb770e58c1744a1750169f9e399c9889270a8d33141a242a596701c2cdf2b95ai0",
        "output_value": 546,
        "parent": null,
        "previous": "ef26436acccaa198baa5c6fa899ec4b8a4faabf61400e92cc3193dac33e30856i0",
        "rune": null,
        "sat": 621497399984255,
        "satpoint": "640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59:0:0",
        "timestamp": 1703018855
    }
}
```

</details>

### Inscription by Number

Returns inscription information based on an inscription number.

{% tabs %}
{% tab title="curl" %}
```
curl -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_inscription",
    "params": [
      "802285"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`insc_num`**: string (required) - The inscription number

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "address": "bc1p2zzyghqct8ls7l0ash3xu57fm83djakepgyr8tuzmuaw995dpadsrn68q3",
    "children": [],
    "content_length": 50,
    "content_type": "text/plain;charset=utf-8",
    "genesis_fee": 848,
    "genesis_height": 783768,
    "inscription_id": "4a45b966dca8e28dc5f1c2a84911e7655e5a90e3bd72cff086cd6f9290e43c90i0",
    "inscription_number": 802285,
    "next": "212e180a7d1ef0951dc382a1aaf4233fa455bd32aaab28e65fc31a906b2fa290i0",
    "output_value": 546,
    "parent": null,
    "previous": "17aa0b96285cd2ba36c41d21d27e115e2484404684d21dbc687b88166859868ei0",
    "rune": null,
    "sat": 705392790218085,
    "satpoint": "4a45b966dca8e28dc5f1c2a84911e7655e5a90e3bd72cff086cd6f9290e43c90:0:0",
    "timestamp": 1680528792
  }
}
```

</details>

### Inscription Children

Returns the first 100 child inscription IDs for the specified inscription.

With the `page` parameter, it returns the set of 100 child inscriptions on `page`.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_r:children",
    "params": [
      "60bcf821240064a9c55225c4f01711b0ebbcab39aa3fafeefe4299ab158536fai0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`inscription_id`**: string (required) - The inscription ID

**`page`**: string (optional) - Return inscriptions from this page

</details>

<details>

<summary>Response</summary>

```
```

</details>

### Inscription by Sat

Returns the inscription IDs for a specific Sat number.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_r:sat",
    "params": [
      "1596764664144241"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`sat_number`**: string (required) - Position number of Sat

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "ids": [
            "d1e0e792195d9da7be7ce46ec81fe31f7942e350a5da2baf4a7cffe91adb3fa1i0"
        ],
        "more": false,
        "page": 0
    }
}
```

</details>

### Inscription by Sat with Index

Returns the inscription ID at `index` from all inscriptions on the Sat.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_r:sat::at",
    "params": [
      "1596764664144241"
      "0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`sat_number`**: string (required) - Position number of sat

**`index`**: string (optional) - The index of the inscription. `index` may be a negative number to index from the back (`0` being the first and `-1` being the most recent).

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "id": "d1e0e792195d9da7be7ce46ec81fe31f7942e350a5da2baf4a7cffe91adb3fa1i0"
    }
}
```

</details>

## Sats

### Sat by Number

Returns a Sat by its integer position number within the entire bitcoin supply.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_sat",
    "params": [
      "1596764664144241"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`sat_number`**: string (required) - Position number of Sat

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "number": 1596764664144241,
    "decimal": "437411.914144241",
    "degree": "0°17411′1955″914144241‴",
    "name": "cnqughttwqi",
    "block": 437411,
    "cycle": 0,
    "epoch": 2,
    "period": 216,
    "offset": 914144241,
    "rarity": "common",
    "percentile": "76.03641266193728%",
    "satpoint": "22a0d4ad3fafb1eb53823b7655103bb7d6d7b61e9ac572e2f493bbdb8a371a09:0:0",
    "timestamp": 1478322980,
    "inscriptions": [
      "d1e0e792195d9da7be7ce46ec81fe31f7942e350a5da2baf4a7cffe91adb3fa1i0"
    ]
  }
}
```

</details>

### Sat by Decimal

Returns a Sat by decimal: its block and offset within that block.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_sat",
    "params": [
      "481824.0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block.offset`**: string (required) - Block and offset of Sat

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "number": 481824,
    "decimal": "0.481824",
    "degree": "0°0′0″481824‴",
    "name": "nvtdijtvmrx",
    "block": 0,
    "cycle": 0,
    "epoch": 0,
    "period": 0,
    "offset": 481824,
    "rarity": "common",
    "percentile": "0.000000022944000025238412%",
    "satpoint": null,
    "timestamp": 1231006505,
    "inscriptions": []
  }
}
```

</details>

### Sat by Degree

Returns a Sat by degree: its cycle, blocks since the last halving, blocks since the last difficulty adjustment, and offset within their block.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_sat",
    "params": [
      "1°0′0″0‴"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`cycle`**`°`**`blocks_since_halving`**`′`**`blocks_since_adjustment`**`″`**`offset`**`‴`: string (required) - Cycle, blocks since the last halving, blocks since the last difficulty adjustment, and offset within  block

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "number": 2067187500000000,
    "decimal": "1260000.0",
    "degree": "1°0′0″0‴",
    "name": "fachfvytgb",
    "block": 1260000,
    "cycle": 1,
    "epoch": 6,
    "period": 625,
    "offset": 0,
    "rarity": "legendary",
    "percentile": "98.4375001082813%",
    "satpoint": null,
    "timestamp": 1965864846,
    "inscriptions": []
  }
}
```

</details>

### Sat by Name

Returns a Sat by its base 26 representation using the letters "a" through "z".

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_sat",
    "params": [
      "ahistorical"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`base26_name`**: string (required) - Base 26 representation of Sat

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "number": 1913358459708310,
    "decimal": "751373.334708310",
    "degree": "0°121373′1421″334708310‴",
    "name": "ahistorical",
    "block": 751373,
    "cycle": 0,
    "epoch": 3,
    "period": 372,
    "offset": 334708310,
    "rarity": "common",
    "percentile": "91.1123077053812%",
    "satpoint": null,
    "timestamp": 1661604907,
    "inscriptions": []
  }
}
```

</details>

### Sat by Percentile

Returns a Sat by its percentile: the percentage of bitcoin's supply that has been or will have been issued when they are mined

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_sat",
    "params": [
      "80%"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`percentile`**: string (required) - Sat percentile

</details>

<details>

<summary>Response</summary>

```
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "number": 1679999998151999,
    "decimal": "503999.1248151999",
    "degree": "0°83999′2015″1248151999‴",
    "name": "byifavydtly",
    "block": 503999,
    "cycle": 0,
    "epoch": 2,
    "period": 249,
    "offset": 1248151999,
    "rarity": "common",
    "percentile": "80%",
    "satpoint": null,
    "timestamp": 1515827472,
    "inscriptions": []
  }
}
```

</details>

## Ordinals

### Ord Content

Returns the Base-64 content of an ordinals inscription.

{% tabs %}
{% tab title="curl" %}
```json
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_content",
    "params": [
      "640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59i0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`insc_num`**: string (required) - The inscription number

</details>

<details>

<summary>Response</summary>

{% code overflow="wrap" %}
```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": "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"
}
```
{% endcode %}

</details>

### Ord Preview

Returns HTML for displaying ordinal content.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_preview",
    "params": [
      "640e8ee134ecf886a874bbfd555b9e5beaf70cdc93ffe52cc10f009c8ee1cc59i0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`inscription_id`**: string (required) - The inscription ID

</details>

<details>

<summary>Response</summary>

{% code overflow="wrap" %}
```
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": "<!doctype html>\n<html lang=en>\n  <head>\n    <meta charset=utf-8>\n    <meta name=format-detection content='telephone=no'>\n    <style>\n      html {\n        background-color: #131516;\n        height: 100%;\n      }\n\n      body {\n        background-image: url(/content/85a3c4a4b94f2cd83db4b075553bd92011cefc5816cd76b36375ec34ecd4e57ei0);\n        background-position: center;\n        background-repeat: no-repeat;\n        background-size: contain;\n        height: 100%;\n        image-rendering: pixelated;\n        margin: 0;\n      }\n\n      img {\n        height: 100%;\n        opacity: 0;\n        width: 100%;\n      }\n    </style>\n  </head>\n  <body>\n    <img src=/content/85a3c4a4b94f2cd83db4b075553bd92011cefc5816cd76b36375ec34ecd4e57ei0></img>\n  </body>\n</html>\n"
}
```
{% endcode %}

</details>

##

## Recursive Methods

Recursive inscriptions facilitate the access and incorporation of existing inscription data into new inscriptions. By leveraging recursive inscriptions, users can effectively 'daisy-chain' data, pulling information from a series of interconnected inscriptions

More information can be found in our [Inscription Guide](../guides/inscriptions.md).

### Child Inscriptions

`ord_r:children` returns the first 100 child inscription IDs. If called with the optional page parameter, the set of 100 child inscriptions on `page` will be returned.

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_r:children",
    "params": [
      "60bcf821240064a9c55225c4f01711b0ebbcab39aa3fafeefe4299ab158536fai0"
      "2"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`inscription_id`**: string (required) - The inscription ID

**`page`**: string (optional) - The page number of the child inscriptions.

</details>

<details>

<summary>Response</summary>

<pre class="language-json" data-overflow="wrap"><code class="lang-json">{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "ids": [            "7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i82",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i83",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i84",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i85",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i86",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i87",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i88",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i89",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i90",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i91",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i92",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i93",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i94",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i95",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i96",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i97",
"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i98",
<strong>"7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i99"
</strong>        ],
        "more": true,
        "page": 0
    }
}
</code></pre>

</details>

### Sat Inscriptions

`ord_r:sat` returns the first 100 inscriptions at `sat_number`. If called with the optional page parameter, the set of 100 inscriptions on `page` will be returned.

`<sat_number>` only allows the actual number of a sat no other sat notations like degree, percentile or decimal.&#x20;

{% tabs %}
{% tab title="curl" %}
```
curl -s -XPOST https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "ord_r:sat",
    "params": [
      "1897135510679085",
      "0"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`sat_number`**: string (required) - The inscription ID

**`page`**: string (optional) - The page number of the child inscriptions.

</details>

<details>

<summary>Response</summary>

{% code overflow="wrap" %}
```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "ids": [
            "7cd66b8e3a63dcd2fada917119830286bca0637267709d6df1ca78d98a1b4487i200"
        ],
        "more": false,
        "page": 0
    }
}
```
{% endcode %}

</details>
