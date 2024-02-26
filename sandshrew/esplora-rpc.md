# Esplora RPC

The Sandshrew Esplora RPC API represents a significant advancement in Bitcoin blockchain indexing and querying, offering a robust enhancement to the Bitcoin Core RPC. Its design focuses on indexing the entire Bitcoin blockchain, allowing for swift and effective data queries. This makes Sandshrew's Esplora RPC particularly valuable for applications that need real-time tracking of wallets, monitoring balances and transactions with high efficiency. Developers crafting Bitcoin-centric applications or services find Esplora's capabilities essential, as it delivers a more comprehensive and user-friendly interface for accessing blockchain data.

**Key Features of the Esplora API:**

* **Comprehensive Transaction Details**: Esplora provides in-depth information about transactions, covering aspects like inputs, outputs, fees, size, and more.
* **Detailed Block Information**: The API delivers extensive data on blocks, including height, list of transactions, timestamps, size, and other vital metrics.
* **Real-Time Address and Scripthash Data**: It enables the retrieval of detailed information on specific Bitcoin addresses or script hashes. This feature is crucial for tracking transaction histories and UTXOs, facilitating real-time monitoring of wallet balances and activities.
* **Mempool Statistics**: Esplora offers insights into the mempool's current status, including the number of pending transactions, their cumulative size, and associated fees.
* **Blockchain Statistics**: The API includes endpoints for various blockchain statistics like the most recent block, current block height, and fee estimates for different confirmation timescales.

Sandshrew's Esplora RPC indexer is a great option for developers needing rapid, accurate data access. Esplora surpasses traditional Bitcoin Core RPC capabilities, providing a scalable and efficient solution for a range of needs, from wallet balance tracking to transaction status checks and broader blockchain data analysis.

### Sending Transactions to the Esplora RPC Service

Interfacing with the Ordinals RPC service requires developers to[ connect with Oyl and get a developer key](../welcome/getting-started.md).

All interactions with the Oyl Developer API follow a [standard RPC Query format](sandshrew-rpc.md).

## Transactions <a href="#user-content-transactions" id="user-content-transactions"></a>

### Tx <a href="#user-content-transactions" id="user-content-transactions"></a>

`esplora_tx` returns detailed information about a specific Bitcoin transaction based on its transaction ID (`txid`). It provides comprehensive data about the transaction, including inputs, outputs, block information, and any associated inscriptions.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx",
    "params": [
      "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "txid": "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c",
    "version": 1,
    "locktime": 0,
    "vin": [
      {
        "txid": "0000000000000000000000000000000000000000000000000000000000000000",
        "vout": 4294967295,
        "prevout": null,
        "scriptsig": "03367b0c1b4d696e656420627920416e74506f6f6c39373102000101ca9a6d79fabe6d6df565fc9c90c3d1f40e471384b84d69e5720588aea827622e22ec662d5188380e10000000000000000000e3eac000000000000000",
        "scriptsig_asm": "OP_PUSHBYTES_3 367b0c OP_PUSHBYTES_27 4d696e656420627920416e74506f6f6c39373102000101ca9a6d79 OP_RETURN_250 OP_RETURN_190 OP_2DROP OP_2DROP OP_RETURN_245 OP_VERIF OP_RETURN_252 OP_NUMEQUAL OP_ABS OP_RETURN_195 OP_RETURN_209 OP_RETURN_244 OP_PUSHBYTES_14 471384b84d69e5720588aea82762 OP_PUSHBYTES_46 <push past end>",
        "witness": [
          "0000000000000000000000000000000000000000000000000000000000000000"
        ],
        "is_coinbase": true,
        "sequence": 4294967295
      }
    ],
    "vout": [
      {
        "scriptpubkey": "a9144b09d828dfc8baaba5d04ee77397e04b1050cc7387",
        "scriptpubkey_asm": "OP_HASH160 OP_PUSHBYTES_20 4b09d828dfc8baaba5d04ee77397e04b1050cc73 OP_EQUAL",
        "scriptpubkey_type": "p2sh",
        "scriptpubkey_address": "38XnPvu9PmonFU9WouPXUjYbW91wa5MerL",
        "value": 726160425
      },
      {
        "scriptpubkey": "6a24aa21a9ed0fdf8d12166cec30ab3d4f99a9889229aed843a20cd7d8459b08e2010d3d2656",
        "scriptpubkey_asm": "OP_RETURN OP_PUSHBYTES_36 aa21a9ed0fdf8d12166cec30ab3d4f99a9889229aed843a20cd7d8459b08e2010d3d2656",
        "scriptpubkey_type": "op_return",
        "value": 0
      },
      {
        "scriptpubkey": "6a2d434f5245012953559db5cc88ab20b1960faa9793803d0703375997be5a09d05bb9bac27ec60419d0b373f32b20",
        "scriptpubkey_asm": "OP_RETURN OP_PUSHBYTES_45 434f5245012953559db5cc88ab20b1960faa9793803d0703375997be5a09d05bb9bac27ec60419d0b373f32b20",
        "scriptpubkey_type": "op_return",
        "value": 0
      },
      {
        "scriptpubkey": "6a2952534b424c4f434b3ab29c71f95f884e91c2b30efa14e824f3f15bfabd76a58ea791b833270059089a",
        "scriptpubkey_asm": "OP_RETURN OP_PUSHBYTES_41 52534b424c4f434b3ab29c71f95f884e91c2b30efa14e824f3f15bfabd76a58ea791b833270059089a",
        "scriptpubkey_type": "op_return",
        "value": 0
      }
    ],
    "size": 362,
    "weight": 1340,
    "fee": 0,
    "status": {
      "confirmed": true,
      "block_height": 817974,
      "block_hash": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
      "block_time": 1700669241
    }
  }
}
```

</details>

### Tx Status <a href="#user-content-get-txtxidstatus" id="user-content-get-txtxidstatus"></a>

Returns the transaction confirmation status.

Available fields: `confirmed` (boolean), `block_height` (optional) and `block_hash` (optional).

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::status",
    "params": [
      "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "confirmed": true,
    "block_height": 817974,
    "block_hash": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
    "block_time": 1700669241
  }
}
```

</details>

### Tx Hex <a href="#user-content-get-txtxidhex" id="user-content-get-txtxidhex"></a>

Returns the hex encoded transaction.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::hex",
    "params": [
      "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff5803367b0c1b4d696e656420627920416e74506f6f6c39373102000101ca9a6d79fabe6d6df565fc9c90c3d1f40e471384b84d69e5720588aea827622e22ec662d5188380e10000000000000000000e3eac000000000000000ffffffff042954482b0000000017a9144b09d828dfc8baaba5d04ee77397e04b1050cc73870000000000000000266a24aa21a9ed0fdf8d12166cec30ab3d4f99a9889229aed843a20cd7d8459b08e2010d3d265600000000000000002f6a2d434f5245012953559db5cc88ab20b1960faa9793803d0703375997be5a09d05bb9bac27ec60419d0b373f32b2000000000000000002b6a2952534b424c4f434b3ab29c71f95f884e91c2b30efa14e824f3f15bfabd76a58ea791b833270059089a0120000000000000000000000000000000000000000000000000000000000000000000000000"
}
```

</details>

### Tx Raw <a href="#user-content-get-txtxidraw" id="user-content-get-txtxidraw"></a>

Returns the raw transaction as binary data.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::raw",
    "params": [
      "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

**`verbose`**: string (optional, defaults to "false") - If "false" return a raw string, otherwise return a json object

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "<raw binary>"
}
```

</details>

### Tx Merkleblock Proof <a href="#user-content-get-txtxidmerkleblock-proof" id="user-content-get-txtxidmerkleblock-proof"></a>

Returns a merkle inclusion proof for the transaction using merkleblock format.

_Note:_ This endpoint is not currently available for Liquid/Elements-based chains.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::merkleblock-proof",
    "params": [
      "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0000b72dfdb7b7c44d9d79ca56a805182c72045d89897a50058e01000000000000000000f9cc47e793a7519e612d5203e2d40c3da9c6ded9c5f921ca90fbb82625b2dca139275e65125a0417a0e5b09f4b1000000e2c4ac8db2401f4ce3358c1c6c8a9cf8e6dd990ff5f0763a902a1d95f633285ee868e00921348e0af4810c3984a60aaaeb3bce95b6bc21bcedbd033de7c5616949ff54b59178253cb278a509a1f66a21348b57887f4168ce5e913c503c5a1209ecc35776cbe94550f5199efcba1b8e8af1f0c48ab5fce8c24a3740aa75065664b95f9cec95ccffa211f319132bc63ff89f00559245128aa4feb35356bf9bd1bf685597880fbef24bf5ebbfe7b95e562956f5e77b1fe17029ecaee92c43ec355b6b90fd87d1909a065eb9783e5e76b96704c2a4e16607a907d6e55ecc2bb40996b0dd873dda2d17a8a7438e627689a563898ce105f5f85d19c86cea44d2e21ed9312e05a12e74eae1c2faebae5a59dfe783a173382ec4b23dfcdf5df26b4bfb1a3d5781440e661cb0c2b7c48f71387056d24a24556e7193d04e1096afb6c64a2fd05a64d00956dcdb4e06be05169e99b8ec4fc6f569ddc20861b3b351fa997b7e9dce729de9641c89fb493e4a88c1ec713f5a54586c53bdd07e8247a71119b93ea7a694a2858cbb53889ac4ff4ac104f61e3450a1b793d04d664de0f3f7b22f5791d3ca263b3db3e678cc4f02ffd5bcb0052cc2f39276cadf0c16607d626b0e6d304ff3f0000"
}
```

</details>

### Tx Merkle-proof <a href="#user-content-get-txtxidmerkle-proof" id="user-content-get-txtxidmerkle-proof"></a>

Returns a merkle inclusion proof for the transaction  with the following keys:

*   _block\_height_

    The height of the block the transaction was confirmed in.
*   _merkle_

    A list of transaction hashes the current hash is paired with, recursively, in order to trace up to obtain merkle root of the block, deepest pairing first.
*   _pos_

    The 0-based index of the position of the transaction in the ordered list of transactions in the block.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::merkle-proof",
    "params": [
      "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "block_height": 817974,
    "merkle": [
      "9416567cde33d0dbce1bc26b5be9bcb3aeaa604a98c31048afe0481392008e86",
      "9e20a1c503c513e9e58c16f48778b54813a2661f9a508a27cb538217594bf59f",
      "4b666550a70a74a3248cce5fab480c1fafe8b8a1cbef99510f5594be6c7735cc",
      "f61bbdf96b3535eb4faa2851245905f089ff63bc3291311f21facf5cc9cef995",
      "b655c33ec492eeca9e0217feb1775e6f9562e5957bfebb5ebf24effb80785985",
      "6b9940bbc2ec556e7d907a60164e2a4c70966be7e58397eb65a009197dd80fb9",
      "93ed212e4da4ce869cd1855f5f10ce9838569a6827e638748a7ad1a2dd73d80d",
      "a3b1bfb426dff5cddf234bec8233173a78fe9da5e5baae2f1cae4ee7125ae012",
      "fda2646cfb6a09e1043d19e75645a2246d058713f7487c2b0ccb61e6401478d5",
      "e9b797a91f353b1b8620dc9d566ffcc48e9be96951e06be0b4cd6d95004da605",
      "ea939b11717a24e807dd3bc58645a5f513c71e8ca8e493b49fc84196de29e7dc",
      "79f5227b3f0fde64d6043d791b0a45e3614f10acf44fac8938b5cb58284a697a",
      "d3e6b026d60766c1f0ad6c27392fcc5200cb5bfd2ff0c48c673edbb363a23c1d"
    ],
    "pos": 0
  }
}
```

</details>

### Tx Outspends

Returns the spending status of all transaction outputs.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::outspends",
    "params": [
      "b0ba58a69a7b11dfac81351af9c011bd52168346e8cc3311e1282e335a4728fb"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "spent": false
    },
    {
      "spent": false
    },
    {
      "spent": true,
      "txid": "9416567cde33d0dbce1bc26b5be9bcb3aeaa604a98c31048afe0481392008e86",
      "vin": 0,
      "status": {
        "confirmed": true,
        "block_height": 817974,
        "block_hash": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
        "block_time": 1700669241
      }
    },
    {
      "spent": false
    },
    {
      "spent": false
    },
    {
      "spent": true,
      "txid": "6ef9ea136a0dfddb31b75b2363b640425e71ef47b4a21e5b95a054c47e3b4449",
      "vin": 4,
      "status": {
        "confirmed": true,
        "block_height": 817817,
        "block_hash": "000000000000000000038e07b5e8eaa8bd723fba908fe73019f730d4f503a049",
        "block_time": 1700582788
      }
    }
  ]
}
```

</details>

### Tx Outspend <a href="#tx-outspend" id="tx-outspend"></a>

Returns the spending status of a specific transaction output.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_tx::outspend",
    "params": [
      "b0ba58a69a7b11dfac81351af9c011bd52168346e8cc3311e1282e335a4728fb",
      "2"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`txid`**: string (required) - Transaction ID

**`vout`**: string (required) - The number of the transaction output

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "spent": true,
    "txid": "9416567cde33d0dbce1bc26b5be9bcb3aeaa604a98c31048afe0481392008e86",
    "vin": 0,
    "status": {
      "confirmed": true,
      "block_height": 817974,
      "block_hash": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
      "block_time": 1700669241
    }
  }
}
```

</details>

## Addresses <a href="#user-content-addresses" id="user-content-addresses"></a>

### Address Info

Returns details about an address.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_address",
    "params": [
      "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`address`**: string (required) - Bitcoin address

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "address": "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7",
    "chain_stats": {
      "funded_txo_count": 5,
      "funded_txo_sum": 9141224,
      "spent_txo_count": 2,
      "spent_txo_sum": 5975682,
      "tx_count": 5
    },
    "mempool_stats": {
      "funded_txo_count": 0,
      "funded_txo_sum": 0,
      "spent_txo_count": 0,
      "spent_txo_sum": 0,
      "tx_count": 0
    }
  }
}
```

</details>

### Address Tx History <a href="#user-content-get-addressaddresstxs" id="user-content-get-addressaddresstxs"></a>

Get transaction history for the specified address. Returns up to 50 mempool transactions plus the first 25 confirmed transactions. You can request more confirmed transactions using `last_seen_txid`.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_address::txs",
    "params": [
      "bc1pqu8j3dlfwjkzt6tcx6w2dvf9j4wxku2n7mnp9eawegayu6k6x9xsgsff7n"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`address`**: string (required) - Bitcoin address

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "txid": "f719010042cda872a6add2f98a51c3e0f9997a6d9cd92c321c8dab9ef69350be",
      "version": 2,
      "locktime": 0,
      "vin": [
        {
          "txid": "23ac1179890cfed9d3b9cdb290ad4c98f890ec0781a3de1f7600ee5d48e06853",
          "vout": 0,
          "prevout": {
            "scriptpubkey": "5120070f28b7e974ac25e978369ca6b125955c6b7153f6e612e7aeca3a4e6ada314d",
            "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 070f28b7e974ac25e978369ca6b125955c6b7153f6e612e7aeca3a4e6ada314d",
            "scriptpubkey_type": "v1_p2tr",
            "scriptpubkey_address": "bc1pqu8j3dlfwjkzt6tcx6w2dvf9j4wxku2n7mnp9eawegayu6k6x9xsgsff7n",
            "value": 16000
          },
          "scriptsig": "",
          "scriptsig_asm": "",
          "witness": [
            "e0e706d5cfbf1d8cea1d8ffecb1b00f99897f293002660b23ded35fb35144aade62227a49ad634e916cdff6ca57620a1c207ae53738f35292e341e7e865264cb",
            "20fc19540a13a609121f98a939348df194fbf59c6ffa8f5aec099207dc14a395aeac0063036f7264010118746578742f706c61696e3b636861727365743d7574662d3800357b2270223a226272632d3230222c226f70223a226d696e74222c227469636b223a22746f696c222c22616d74223a2231303030227d68",
            "c151cae3bc8e151ca21c4a093362d6aab05a25bb5693cdf5a6861cf5eb59402272"
          ],
          "is_coinbase": false,
          "sequence": 4294967293
        }
      ],
      "vout": [
        {
          "scriptpubkey": "51201ef4bb672ee1bc1014c72f7ead247e5e495a1c35ed61376ce1b68f1e1f99d400",
          "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 1ef4bb672ee1bc1014c72f7ead247e5e495a1c35ed61376ce1b68f1e1f99d400",
          "scriptpubkey_type": "v1_p2tr",
          "scriptpubkey_address": "bc1prm6tkeewux7pq9x89al26fr7tey458p4a4snwm8pk683u8ue6sqqekcchr",
          "value": 546
        },
        {
          "scriptpubkey": "001488e946652af2729f12abc03120387d152e9c22fa",
          "scriptpubkey_asm": "OP_0 OP_PUSHBYTES_20 88e946652af2729f12abc03120387d152e9c22fa",
          "scriptpubkey_type": "v0_p2wpkh",
          "scriptpubkey_address": "bc1q3r55vef27fef7y4tcqcjqwraz5hfcgh6yl645u",
          "value": 2532
        }
      ],
      "size": 351,
      "weight": 726,
      "fee": 12922,
      "status": {
        "confirmed": true,
        "block_height": 817831,
        "block_hash": "000000000000000000013873daaff66f8ec5b48075babbfcccac831a00c8c767",
        "block_time": 1700589870
      }
    },
    {
      "txid": "23ac1179890cfed9d3b9cdb290ad4c98f890ec0781a3de1f7600ee5d48e06853",
      "version": 2,
      "locktime": 0,
      "vin": [
        {
          "txid": "9d92bea825ec5736f7cb0bb8b952b6bb0b7dc6f13f300245b2aece25acb31ef8",
          "vout": 1,
          "prevout": {
            "scriptpubkey": "00142bfe42a69e37751a96776c8151391c055ca1b460",
            "scriptpubkey_asm": "OP_0 OP_PUSHBYTES_20 2bfe42a69e37751a96776c8151391c055ca1b460",
            "scriptpubkey_type": "v0_p2wpkh",
            "scriptpubkey_address": "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7",
            "value": 2975682
          },
          "scriptsig": "",
          "scriptsig_asm": "",
          "witness": [
            "3045022100c4be8d0a8ef2d0179fe7ccb2c36f37ffbe75dd9632da1f2d6bd2ac2326dc8ebc022021066387ff665c7e2072bca8c3c1324795edd8a5885208d3ef002af4b7179f1a01",
            "026de43f1d41eeba32b31ad449edb28ef0390b97828d7c76f61cbcc8cc4de64123"
          ],
          "is_coinbase": false,
          "sequence": 4294967295
        }
      ],
      "vout": [
        {
          "scriptpubkey": "5120070f28b7e974ac25e978369ca6b125955c6b7153f6e612e7aeca3a4e6ada314d",
          "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 070f28b7e974ac25e978369ca6b125955c6b7153f6e612e7aeca3a4e6ada314d",
          "scriptpubkey_type": "v1_p2tr",
          "scriptpubkey_address": "bc1pqu8j3dlfwjkzt6tcx6w2dvf9j4wxku2n7mnp9eawegayu6k6x9xsgsff7n",
          "value": 16000
        },
        {
          "scriptpubkey": "00142bfe42a69e37751a96776c8151391c055ca1b460",
          "scriptpubkey_asm": "OP_0 OP_PUSHBYTES_20 2bfe42a69e37751a96776c8151391c055ca1b460",
          "scriptpubkey_type": "v0_p2wpkh",
          "scriptpubkey_address": "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7",
          "value": 2948765
        }
      ],
      "size": 235,
      "weight": 610,
      "fee": 10917,
      "status": {
        "confirmed": true,
        "block_height": 817831,
        "block_hash": "000000000000000000013873daaff66f8ec5b48075babbfcccac831a00c8c767",
        "block_time": 1700589870
      }
    }
  ]
}
```

</details>

### Address Confirmed Tx

Get confirmed transaction history for the specified address, sorted with newest first. Returns 25 transactions per page. More can be requested by specifying the `last_seen_txid` seen by the previous query.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_address::txs:chain",
    "params": [
      "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`address`**: string (required) - Bitcoin address

**`last_seen_txid`**: string (optional) - Transaction ID from previous query

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "txid": "23ac1179890cfed9d3b9cdb290ad4c98f890ec0781a3de1f7600ee5d48e06853",
      "version": 2,
      "locktime": 0,
      "vin": [
        {
          "txid": "9d92bea825ec5736f7cb0bb8b952b6bb0b7dc6f13f300245b2aece25acb31ef8",
          "vout": 1,
          "prevout": {
            "scriptpubkey": "00142bfe42a69e37751a96776c8151391c055ca1b460",
            "scriptpubkey_asm": "OP_0 OP_PUSHBYTES_20 2bfe42a69e37751a96776c8151391c055ca1b460",
            "scriptpubkey_type": "v0_p2wpkh",
            "scriptpubkey_address": "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7",
            "value": 2975682
          },
          "scriptsig": "",
          "scriptsig_asm": "",
          "witness": [
            "3045022100c4be8d0a8ef2d0179fe7ccb2c36f37ffbe75dd9632da1f2d6bd2ac2326dc8ebc022021066387ff665c7e2072bca8c3c1324795edd8a5885208d3ef002af4b7179f1a01",
            "026de43f1d41eeba32b31ad449edb28ef0390b97828d7c76f61cbcc8cc4de64123"
          ],
          "is_coinbase": false,
          "sequence": 4294967295
        }
      ],
      "vout": [
        {
          "scriptpubkey": "5120070f28b7e974ac25e978369ca6b125955c6b7153f6e612e7aeca3a4e6ada314d",
          "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 070f28b7e974ac25e978369ca6b125955c6b7153f6e612e7aeca3a4e6ada314d",
          "scriptpubkey_type": "v1_p2tr",
          "scriptpubkey_address": "bc1pqu8j3dlfwjkzt6tcx6w2dvf9j4wxku2n7mnp9eawegayu6k6x9xsgsff7n",
          "value": 16000
        },
        {
          "scriptpubkey": "00142bfe42a69e37751a96776c8151391c055ca1b460",
          "scriptpubkey_asm": "OP_0 OP_PUSHBYTES_20 2bfe42a69e37751a96776c8151391c055ca1b460",
          "scriptpubkey_type": "v0_p2wpkh",
          "scriptpubkey_address": "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7",
          "value": 2948765
        }
      ],
      "size": 235,
      "weight": 610,
      "fee": 10917,
      "status": {
        "confirmed": true,
        "block_height": 817831,
        "block_hash": "000000000000000000013873daaff66f8ec5b48075babbfcccac831a00c8c767",
        "block_time": 1700589870
      }
    }
  ]
}
```

</details>

### Addr Mempool Tx&#x20;

Get unconfirmed transaction history for the specified address. Returns up to 50 transactions (no paging).

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_address::txs:mempool",
    "params": [
      "bc1pqu8j3dlfwjkzt6tcx6w2dvf9j4wxku2n7mnp9eawegayu6k6x9xsgsff7n"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`address`**: string (required) - Bitcoin address

</details>

### Addr UTXOs

Get the list of unspent transaction outputs associated with the address.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_address::utxo",
    "params": [
      "bc1q90ly9f57xa6349nhdjq4zwguq4w2rdrqgfgtd7"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`address`**: string (required) - Bitcoin address

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "txid": "32c99ef2048481d70040c7338e50bc9682de389506a55448b051ef8d893aee6e",
      "vout": 0,
      "status": {
        "confirmed": true,
        "block_height": 817824,
        "block_hash": "000000000000000000036133fc5e75e5bb3656f2aaedb6f7a06e1997e1627bd7",
        "block_time": 1700585869
      },
      "value": 546
    },
    {
      "txid": "23ac1179890cfed9d3b9cdb290ad4c98f890ec0781a3de1f7600ee5d48e06853",
      "vout": 1,
      "status": {
        "confirmed": true,
        "block_height": 817831,
        "block_hash": "000000000000000000013873daaff66f8ec5b48075babbfcccac831a00c8c767",
        "block_time": 1700589870
      },
      "value": 2948765
    },
    {
      "txid": "37a5e179b008f956bd00a3e07444f9cc747d6b9deaaa05b8eacd073b3108f4e2",
      "vout": 10,
      "status": {
        "confirmed": true,
        "block_height": 816621,
        "block_hash": "0000000000000000000355718a4955b850b1c5b17db6ae6a01962245b5ab7149",
        "block_time": 1699894589
      },
      "value": 216231
    }
  ]
}
```

</details>

## Scripthash <a href="#user-content-addresses" id="user-content-addresses"></a>

A [_script hash_](https://electrumx.readthedocs.io/en/latest/protocol-basics.html#script-hashes) is the hash of the binary bytes of the locking script (ScriptPubKey), expressed as a hexadecimal string. The hash function to use is currently `sha256()`.&#x20;

For example, the legacy Bitcoin address from the genesis block:

<pre class="language-javascript"><code class="lang-javascript"><strong>const address = '1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa'
</strong></code></pre>

has P2PKH scriptpubkey:

<pre class="language-javascript"><code class="lang-javascript">const script = bitcoin.address.toOutputScript(address)
console.log('script.toString('hex'))

<strong>// 76a91462e907b15cbf27d5425399ebf6f0fb50ebb88f1888ac
</strong></code></pre>

with SHA256 hash, the `scripthash`:

```javascript
const hash = bitcoin.crypto.sha256(script)
console.log('Scripthash: ', hash.toString('hex'))

// Scripthash: 6191c3b590bfcfa0475e877c302da1e323497acf3b42c08d8fa28e364edf018b
```

which is sent to the server reversed as:

```javascript
const reversedHash = hash.reverse()
console.log(reversedHash.toString('hex'))

// 8b01df4e368ea28f8dc0423bcf7a4923e3a12d307c875e47a0cfbf90b5c39161
```

By subscribing to this hash you can find P2PK payments to the genesis block public key.

### Scripthash Info <a href="#user-content-get-scripthashhash" id="user-content-get-scripthashhash"></a>

Get chain and mempool stats for a scripthash (script pub key).

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_scripthash",
    "params": [
      "6191c3b590bfcfa0475e877c302da1e323497acf3b42c08d8fa28e364edf018b"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`script_hash`**: string (required) - Script hash

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "chain_stats": {
            "funded_txo_count": 6675,
            "funded_txo_sum": 4967680559,
            "spent_txo_count": 0,
            "spent_txo_sum": 0,
            "tx_count": 6620
        },
        "mempool_stats": {
            "funded_txo_count": 1,
            "funded_txo_sum": 2450,
            "spent_txo_count": 0,
            "spent_txo_sum": 0,
            "tx_count": 1
        },
        "scripthash": "6191c3b590bfcfa0475e877c302da1e323497acf3b42c08d8fa28e364edf018b"
    }
}
```

</details>

### Scripthash Tx History <a href="#user-content-get-scripthashhashtxs" id="user-content-get-scripthashhashtxs"></a>

Get transaction history for the specified address/scripthash, sorted with newest first.

Returns up to 50 mempool transactions plus the first 25 confirmed transactions. You can request more confirmed transactions using last\_seen\_txid.

beg

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_scripthash::txs",
    "params": [
      "6191c3b590bfcfa0475e877c302da1e323497acf3b42c08d8fa28e364edf018b"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`script_hash`**: string (required) - Script hash

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": [
        {
            "txid": "255016ede76fad37d082f811ec4355a0b1dae7e9f5ac6c3b707f411ef2c4c046",
            "version": 1,
            "locktime": 0,
            "vin": [
                {
                    "txid": "45e8094a5e906dbd1861771f91c2e0c3020ae565781248139eb6a41d3dc6c5ab",
                    "vout": 1,
                    "prevout": {
                        "scriptpubkey": "5120b59e955d58211345464b668cf9adb82dfa1cbb57ff2e692d18a119ee5d24dcbe",
                        "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 b59e955d58211345464b668cf9adb82dfa1cbb57ff2e692d18a119ee5d24dcbe",
                        "scriptpubkey_type": "v1_p2tr",
                        "scriptpubkey_address": "bc1pkk0f2h2cyyf523jtv6x0ntdc9hapew6hluhxjtgc5yv7uhfymjlq9ff3hq",
                        "value": 10563760
                    },
                    "scriptsig": "",
                    "scriptsig_asm": "",
                    "witness": [
                        "a78f1f170eaa758f00c380a138f39cbf30d4e6a367632a606a6066bea55805c90181f9b2be5a837b694f27dbde8bbba8d1ad4b4edbda494c31603a7c2dc05e47"
                    ],
                    "is_coinbase": false,
                    "sequence": 4294967295
                }
            ],
            "vout": [
                {
                    "scriptpubkey": "76a91462e907b15cbf27d5425399ebf6f0fb50ebb88f1888ac",
                    "scriptpubkey_asm": "OP_DUP OP_HASH160 OP_PUSHBYTES_20 62e907b15cbf27d5425399ebf6f0fb50ebb88f18 OP_EQUALVERIFY OP_CHECKSIG",
                    "scriptpubkey_type": "p2pkh",
                    "scriptpubkey_address": "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
                    "value": 2450
                },
                {
                    "scriptpubkey": "51201e5973cd6884d0159e119f2d809a7f0753449b37c82c348f275cc00ff5ee67f9",
                    "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 1e5973cd6884d0159e119f2d809a7f0753449b37c82c348f275cc00ff5ee67f9",
                    "scriptpubkey_type": "v1_p2tr",
                    "scriptpubkey_address": "bc1prevh8ntgsngpt8s3nukcpxnlqaf5fxeheqkrfre8tnqqla0wvlusm97nr0",
                    "value": 10557660
                }
            ],
            "size": 196,
            "weight": 580,
            "fee": 3650,
            "status": {
                "confirmed": false
            }
        },
        etc...
    ]
}
```

</details>

### Scripthash Confirmed Tx <a href="#user-content-get-scripthashhashtxschainlast_seen_txid" id="user-content-get-scripthashhashtxschainlast_seen_txid"></a>

Get confirmed transaction history for the specified address/scripthash, sorted with newest first.

Returns 25 transactions per page. More can be requested by specifying the last txid seen by the previous query.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_scripthash::txs:chain",
    "params": [
      "6191c3b590bfcfa0475e877c302da1e323497acf3b42c08d8fa28e364edf018b"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`script_hash`**: string (required) - Script hash

**`last_seen_txid`**: string (optional) - Transaction ID from previous query

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": [
        {
            "txid": "bbcf0c04477488800f0495dacc2998733431cc8e15136a58af3c5f8a92731be2",
            "version": 2,
            "locktime": 824790,
            "vin": [
                {
                    "txid": "6db9bf86ce7a290ddf3faa66af7cf5db02bd1b7ad898985b25e50ffccc118fc4",
                    "vout": 1,
                    "prevout": {
                        "scriptpubkey": "5120c6407c0e84d5dfeeb6b235f3fa64e866228628bb16412aa1ec15c4232c5a2be1",
                        "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 c6407c0e84d5dfeeb6b235f3fa64e866228628bb16412aa1ec15c4232c5a2be1",
                        "scriptpubkey_type": "v1_p2tr",
                        "scriptpubkey_address": "bc1pceq8cr5y6h07ad4jxhel5e8gvc3gv29mzeqj4g0vzhzzxtz690ss3vpnjp",
                        "value": 32910
                    },
                    "scriptsig": "",
                    "scriptsig_asm": "",
                    "witness": [
                        "6125622a652f8d09f2ba347c8ab838f37d48fce14156a12fd23be59962ae580aa754e6db7d27dda75d4bac48fd1688c3c8c5bc91ebc7d5328ec6f2d5551019d7"
                    ],
                    "is_coinbase": false,
                    "sequence": 4294967293
                }
            ],
            "vout": [
                {
                    "scriptpubkey": "76a91462e907b15cbf27d5425399ebf6f0fb50ebb88f1888ac",
                    "scriptpubkey_asm": "OP_DUP OP_HASH160 OP_PUSHBYTES_20 62e907b15cbf27d5425399ebf6f0fb50ebb88f18 OP_EQUALVERIFY OP_CHECKSIG",
                    "scriptpubkey_type": "p2pkh",
                    "scriptpubkey_address": "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
                    "value": 16193
                }
            ],
            "size": 153,
            "weight": 408,
            "fee": 16717,
            "status": {
                "confirmed": true,
                "block_height": 824791,
                "block_hash": "0000000000000000000285841a02f0a9de3adb288708cad7df957b88642c8c75",
                "block_time": 1704667936
            }
        },
        etc,,,
    ]
}
```

</details>

### Scripthash Mempool Tx <a href="#user-content-get-scripthashhashtxsmempool" id="user-content-get-scripthashhashtxsmempool"></a>

Get unconfirmed transaction history for the specified address/scripthash.

Returns up to 50 transactions (no paging).

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_scripthash::txs:mempool",
    "params": [
      "6191c3b590bfcfa0475e877c302da1e323497acf3b42c08d8fa28e364edf018b"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`script_hash`**: string (required) - Script hash

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": [
        {
            "txid": "255016ede76fad37d082f811ec4355a0b1dae7e9f5ac6c3b707f411ef2c4c046",
            "version": 1,
            "locktime": 0,
            "vin": [
                {
                    "txid": "45e8094a5e906dbd1861771f91c2e0c3020ae565781248139eb6a41d3dc6c5ab",
                    "vout": 1,
                    "prevout": {
                        "scriptpubkey": "5120b59e955d58211345464b668cf9adb82dfa1cbb57ff2e692d18a119ee5d24dcbe",
                        "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 b59e955d58211345464b668cf9adb82dfa1cbb57ff2e692d18a119ee5d24dcbe",
                        "scriptpubkey_type": "v1_p2tr",
                        "scriptpubkey_address": "bc1pkk0f2h2cyyf523jtv6x0ntdc9hapew6hluhxjtgc5yv7uhfymjlq9ff3hq",
                        "value": 10563760
                    },
                    "scriptsig": "",
                    "scriptsig_asm": "",
                    "witness": [
                        "a78f1f170eaa758f00c380a138f39cbf30d4e6a367632a606a6066bea55805c90181f9b2be5a837b694f27dbde8bbba8d1ad4b4edbda494c31603a7c2dc05e47"
                    ],
                    "is_coinbase": false,
                    "sequence": 4294967295
                }
            ],
            "vout": [
                {
                    "scriptpubkey": "76a91462e907b15cbf27d5425399ebf6f0fb50ebb88f1888ac",
                    "scriptpubkey_asm": "OP_DUP OP_HASH160 OP_PUSHBYTES_20 62e907b15cbf27d5425399ebf6f0fb50ebb88f18 OP_EQUALVERIFY OP_CHECKSIG",
                    "scriptpubkey_type": "p2pkh",
                    "scriptpubkey_address": "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
                    "value": 2450
                },
                {
                    "scriptpubkey": "51201e5973cd6884d0159e119f2d809a7f0753449b37c82c348f275cc00ff5ee67f9",
                    "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 1e5973cd6884d0159e119f2d809a7f0753449b37c82c348f275cc00ff5ee67f9",
                    "scriptpubkey_type": "v1_p2tr",
                    "scriptpubkey_address": "bc1prevh8ntgsngpt8s3nukcpxnlqaf5fxeheqkrfre8tnqqla0wvlusm97nr0",
                    "value": 10557660
                }
            ],
            "size": 196,
            "weight": 580,
            "fee": 3650,
            "status": {
                "confirmed": false
            }
        }
    ]
}
```

</details>

### Scripthash UTXOs <a href="#user-content-get-scripthashhashutxo" id="user-content-get-scripthashhashutxo"></a>

Get the list of unspent transaction outputs associated with the scripthash.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_scripthash::utxo",
    "params": [
      "124ce9c09e50af4913187bd0563bdb319d0d7ccdc48e5d1ca8e7061e14dc6d65"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`script_hash`**: string (required) - Script hash

</details>

<details>

<summary>Response</summary>

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": [
        {
            "txid": "37a5e179b008f956bd00a3e07444f9cc747d6b9deaaa05b8eacd073b3108f4e2",
            "vout": 10,
            "status": {
                "confirmed": true,
                "block_height": 816621,
                "block_hash": "0000000000000000000355718a4955b850b1c5b17db6ae6a01962245b5ab7149",
                "block_time": 1699894589
            },
            "value": 216231
        },
        {
            "txid": "32c99ef2048481d70040c7338e50bc9682de389506a55448b051ef8d893aee6e",
            "vout": 0,
            "status": {
                "confirmed": true,
                "block_height": 817824,
                "block_hash": "000000000000000000036133fc5e75e5bb3656f2aaedb6f7a06e1997e1627bd7",
                "block_time": 1700585869
            },
            "value": 546
        },
        {
            "txid": "f66e5657d73c6a34e04bb9873cf2b32cbaa626163542178cfad2c29d0bb1acdc",
            "vout": 1,
            "status": {
                "confirmed": true,
                "block_height": 818015,
                "block_hash": "00000000000000000002e6710101233b85e9977b8864239edce6ef0a3728993f",
                "block_time": 1700692462
            },
            "value": 2921695
        }
    ]
}
```

</details>

## Blocks <a href="#user-content-blocks" id="user-content-blocks"></a>

### Block Info

Returns information about a block.

The response from this endpoint can be cached indefinitely.

{% tabs %}
{% tab title="curl" %}
<pre class="language-sh"><code class="lang-sh">curl https://signet.sandshrew.io/v1/&#x3C;developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block",
    "params": [
<strong>      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5"
</strong>    ]
}'
</code></pre>
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - Hash of the block

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
    "height": 817974,
    "version": 766967808,
    "timestamp": 1700669241,
    "tx_count": 4171,
    "size": 1671331,
    "weight": 3993538,
    "merkle_root": "a1dcb22526b8fb90ca21f9c5d9dec6a93d0cd4e203522d619e51a793e747ccf9",
    "previousblockhash": "000000000000000000018e05507a89895d04722c1805a856ca799d4dc4b7b7fd",
    "mediantime": 1700666768,
    "nonce": 2679170464,
    "bits": 386161170,
    "difficulty": 64678587803496
  }
}
```

</details>

### Block Header <a href="#user-content-get-blockhashheader" id="user-content-get-blockhashheader"></a>

Returns the hex-encoded block header.

The response from this endpoint can be cached indefinitely.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block::header",
    "params": [
      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - Block hash

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0000b72dfdb7b7c44d9d79ca56a805182c72045d89897a50058e01000000000000000000f9cc47e793a7519e612d5203e2d40c3da9c6ded9c5f921ca90fbb82625b2dca139275e65125a0417a0e5b09f"
}
```

</details>

### Block Status <a href="#user-content-get-blockhashstatus" id="user-content-get-blockhashstatus"></a>

Returns the block status.

Available fields: `in_best_chain` (boolean, false for orphaned blocks), `next_best` (the hash of the next block, only available for blocks in the best chain).

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block::status",
    "params": [
      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - Block hash

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "in_best_chain": true,
    "height": 817974,
    "next_best": "0000000000000000000343455b0ed6eb9929474c2ac72f6ed93788bd314ddaa9"
  }
}
```

</details>

### Tx Data by Block Hash <a href="#user-content-get-blockhashtxsstart_index" id="user-content-get-blockhashtxsstart_index"></a>

Returns a list of transactions in the block (up to 25 transactions beginning at `start_index`).

The response from this endpoint can be cached indefinitely.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block::txs",
    "params": [
      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
      '25'
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - Block hash

**`start_index`**: string (optional) - Start index (must be multiple of 25)

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "txid": "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c",
      "version": 1,
      "locktime": 0,
      "vin": [
        {
          "txid": "0000000000000000000000000000000000000000000000000000000000000000",
          "vout": 4294967295,
          "prevout": null,
          "scriptsig": "03367b0c1b4d696e656420627920416e74506f6f6c39373102000101ca9a6d79fabe6d6df565fc9c90c3d1f40e471384b84d69e5720588aea827622e22ec662d5188380e10000000000000000000e3eac000000000000000",
          "scriptsig_asm": "OP_PUSHBYTES_3 367b0c OP_PUSHBYTES_27 4d696e656420627920416e74506f6f6c39373102000101ca9a6d79 OP_RETURN_250 OP_RETURN_190 OP_2DROP OP_2DROP OP_RETURN_245 OP_VERIF OP_RETURN_252 OP_NUMEQUAL OP_ABS OP_RETURN_195 OP_RETURN_209 OP_RETURN_244 OP_PUSHBYTES_14 471384b84d69e5720588aea82762 OP_PUSHBYTES_46 <push past end>",
          "witness": [
            "0000000000000000000000000000000000000000000000000000000000000000"
          ],
          "is_coinbase": true,
          "sequence": 4294967295
        }
      ],
      "vout": [
        {
          "scriptpubkey": "a9144b09d828dfc8baaba5d04ee77397e04b1050cc7387",
          "scriptpubkey_asm": "OP_HASH160 OP_PUSHBYTES_20 4b09d828dfc8baaba5d04ee77397e04b1050cc73 OP_EQUAL",
          "scriptpubkey_type": "p2sh",
          "scriptpubkey_address": "38XnPvu9PmonFU9WouPXUjYbW91wa5MerL",
          "value": 726160425
        },
        {
          "scriptpubkey": "6a24aa21a9ed0fdf8d12166cec30ab3d4f99a9889229aed843a20cd7d8459b08e2010d3d2656",
          "scriptpubkey_asm": "OP_RETURN OP_PUSHBYTES_36 aa21a9ed0fdf8d12166cec30ab3d4f99a9889229aed843a20cd7d8459b08e2010d3d2656",
          "scriptpubkey_type": "op_return",
          "value": 0
        },
        {
          "scriptpubkey": "6a2d434f5245012953559db5cc88ab20b1960faa9793803d0703375997be5a09d05bb9bac27ec60419d0b373f32b20",
          "scriptpubkey_asm": "OP_RETURN OP_PUSHBYTES_45 434f5245012953559db5cc88ab20b1960faa9793803d0703375997be5a09d05bb9bac27ec60419d0b373f32b20",
          "scriptpubkey_type": "op_return",
          "value": 0
        },
        {
          "scriptpubkey": "6a2952534b424c4f434b3ab29c71f95f884e91c2b30efa14e824f3f15bfabd76a58ea791b833270059089a",
          "scriptpubkey_asm": "OP_RETURN OP_PUSHBYTES_41 52534b424c4f434b3ab29c71f95f884e91c2b30efa14e824f3f15bfabd76a58ea791b833270059089a",
          "scriptpubkey_type": "op_return",
          "value": 0
        }
      ],
      "size": 362,
      "weight": 1340,
      "fee": 0,
      "status": {
        "confirmed": true,
        "block_height": 817974,
        "block_hash": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
        "block_time": 1700669241
      }
    },
    {
      "txid": "9416567cde33d0dbce1bc26b5be9bcb3aeaa604a98c31048afe0481392008e86",
      "version": 2,
      "locktime": 0,
      "vin": [
        {
          "txid": "b0ba58a69a7b11dfac81351af9c011bd52168346e8cc3311e1282e335a4728fb",
          "vout": 2,
          "prevout": {
            "scriptpubkey": "51208ea56cf7a5d036b17128c6b96d05961f2fc53438ee900d93464a215cf2df062c",
            "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 8ea56cf7a5d036b17128c6b96d05961f2fc53438ee900d93464a215cf2df062c",
            "scriptpubkey_type": "v1_p2tr",
            "scriptpubkey_address": "bc1p36jkeaa96qmtzufgc6uk6pvkruhu2dpca6gqmy6xfgs4euklqckqcjy2ke",
            "value": 2049990
          },
          "scriptsig": "",
          "scriptsig_asm": "",
          "witness": [
            "a700383e55e90b43672ba45b732a1f1de9f9edeef92233585ac6899e96ddba9899967cbf5dce1d7e5a02cce81c05d123ddc2c7faa40a1fbeb6351bd72324e547"
          ],
          "is_coinbase": false,
          "sequence": 4294967295
        }
      ],
      "vout": [
        {
          "scriptpubkey": "51209bf64e41fbfac00b07592a9fddf52b7d0b5a0aec360c3247c68359ea1cfa4c37",
          "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 9bf64e41fbfac00b07592a9fddf52b7d0b5a0aec360c3247c68359ea1cfa4c37",
          "scriptpubkey_type": "v1_p2tr",
          "scriptpubkey_address": "bc1pn0myus0mltqqkp6e920amaft05945zhvxcxry37xsdv75886fsmsyu67qe",
          "value": 443405
        },
        {
          "scriptpubkey": "51208ea56cf7a5d036b17128c6b96d05961f2fc53438ee900d93464a215cf2df062c",
          "scriptpubkey_asm": "OP_PUSHNUM_1 OP_PUSHBYTES_32 8ea56cf7a5d036b17128c6b96d05961f2fc53438ee900d93464a215cf2df062c",
          "scriptpubkey_type": "v1_p2tr",
          "scriptpubkey_address": "bc1p36jkeaa96qmtzufgc6uk6pvkruhu2dpca6gqmy6xfgs4euklqckqcjy2ke",
          "value": 1591812
        }
      ],
      "size": 205,
      "weight": 616,
      "fee": 14773,
      "status": {
        "confirmed": true,
        "block_height": 817974,
        "block_hash": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
        "block_time": 1700669241
      }
    },
    ...
  ]
}
```

</details>

### Tx Ids by Block Hash <a href="#user-content-get-blockhashtxids" id="user-content-get-blockhashtxids"></a>

Returns a list of all txids in the block.

The response from this endpoint can be cached indefinitely.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block::txids",
    "params": [
      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - Block hash

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    "ee8532635fd9a102a963075fff90d96d8ecfa9c8c6c15833cef40124dbc84a2c",
    "9416567cde33d0dbce1bc26b5be9bcb3aeaa604a98c31048afe0481392008e86",
    "da01426d48650d29a1c8ead7ea449bd829135aa160ef87089e1488f9fb6a4d8e",
    "c980c83f088aa697d3cb9157309d8eaf67d51147b71a205a197a2fb988d565f0",
    "514dda333fc63caff0c061a038204b1e50e27a2b5de72b28df1125afb06e02e8",
    "0a3e8e42fd2d8e504b589c01eeb5db188a09f0b7cd267eef798a830c0ba84862",
    "625726720bca257d458f2fd4f6686e1cbffafe7c02d26243df7d128f18cbaa02"
  ]
}
```

</details>

### Tx Id by Block Index <a href="#user-content-get-blockhashtxidindex" id="user-content-get-blockhashtxidindex"></a>

Returns the transaction number at index `<index>` within the specified block.

The response from this endpoint can be cached indefinitely.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block::txid",
    "params": [
      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5",
      "3"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`hash`**: string (required) - Block hash

**`index`**: string (optional) - Transaction index

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "c980c83f088aa697d3cb9157309d8eaf67d51147b71a205a197a2fb988d565f0"
}
```

</details>

### Block Raw <a href="#user-content-get-blockhashraw" id="user-content-get-blockhashraw"></a>

Returns the raw block representation in binary.

The response from this endpoint can be cached indefinitely.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block::raw",
    "params": [
      "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_hash`**: string (required) - Block hash

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "<raw binary>"
}
```

</details>

### Block by Height <a href="#user-content-get-block-heightheight" id="user-content-get-block-heightheight"></a>

Returns the hash of the block currently at `block_height`.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_block-height",
    "params": [
      "817974"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`block_height`**: string (required) - Block height

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "000000000000000000006fc88a9a6f7ef0e4b64143482076f3755faef3bfcbd5"
}
```

</details>

### Get Blocks <a href="#user-content-get-blocksstart_height" id="user-content-get-blocksstart_height"></a>

Returns the 10 newest blocks starting at the tip or at `start_height` if specified.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_blocks",
    "params": [
      "817970"
    ]
}'
```
{% endtab %}
{% endtabs %}

<details>

<summary>Params</summary>

**`start_height`**: string (required) - Block height

</details>

<details>

<summary>Response</summary>

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "id": "000000000000000000001eef3949082854a9df683d6627b2c148a338b6cba185",
      "height": 817970,
      "version": 536928256,
      "timestamp": 1700666794,
      "tx_count": 4024,
      "size": 1742334,
      "weight": 3997632,
      "merkle_root": "f9ec5a87109af7b2072af32da7095bcc515d8332ce058e59129f245d1947dbb2",
      "previousblockhash": "00000000000000000003c846d14bde76ed58630801d126f08b8d867945bc35dd",
      "mediantime": 1700664178,
      "nonce": 34040615,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "00000000000000000003c846d14bde76ed58630801d126f08b8d867945bc35dd",
      "height": 817969,
      "version": 777510912,
      "timestamp": 1700666768,
      "tx_count": 4835,
      "size": 1796150,
      "weight": 3993227,
      "merkle_root": "a651f03b6dc31184ce1a98c62ab378aa8a4243f0f3607265ebb90253a539e367",
      "previousblockhash": "000000000000000000043e5d74c30f40202c5aa9edd2b429f98ca5674c7036df",
      "mediantime": 1700663537,
      "nonce": 1265438336,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "000000000000000000043e5d74c30f40202c5aa9edd2b429f98ca5674c7036df",
      "height": 817968,
      "version": 603979776,
      "timestamp": 1700666363,
      "tx_count": 4161,
      "size": 1645171,
      "weight": 3993592,
      "merkle_root": "3f2cc1e99296752dae7df8fa46b8e7a04436accf6f8ae5fae98e64099ffcd2b0",
      "previousblockhash": "00000000000000000001892e06206e660d5b5ebdd27321b19d960b8da0e4ebde",
      "mediantime": 1700663399,
      "nonce": 919130546,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "00000000000000000001892e06206e660d5b5ebdd27321b19d960b8da0e4ebde",
      "height": 817967,
      "version": 681230336,
      "timestamp": 1700665953,
      "tx_count": 4258,
      "size": 1730618,
      "weight": 3993566,
      "merkle_root": "5c6f89f9381f39c63a0faf597a0cdb4c188470dd93554828fb15f3be5824a71a",
      "previousblockhash": "000000000000000000014faef29d21a22ad7e379a6746e46f0c430520209b081",
      "mediantime": 1700663267,
      "nonce": 3784256565,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "000000000000000000014faef29d21a22ad7e379a6746e46f0c430520209b081",
      "height": 817966,
      "version": 661282816,
      "timestamp": 1700665264,
      "tx_count": 3686,
      "size": 1600946,
      "weight": 3997703,
      "merkle_root": "ea9589fba926113800ec8583f7a637c6a40d22b46e19e8da87de3ae5544d4757",
      "previousblockhash": "00000000000000000002e4a146c071db91d9cfc0555c4dfeaa38aa6c961f49dc",
      "mediantime": 1700663138,
      "nonce": 1045846569,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "00000000000000000002e4a146c071db91d9cfc0555c4dfeaa38aa6c961f49dc",
      "height": 817965,
      "version": 536928256,
      "timestamp": 1700664178,
      "tx_count": 3623,
      "size": 1579863,
      "weight": 3997377,
      "merkle_root": "68c45a1de746e8b06e0a798a267eaa9afaad95cc96da964bba47b1dca7f07443",
      "previousblockhash": "0000000000000000000129051d90e404c3121db0c0b8b279fe07659b0e5049cc",
      "mediantime": 1700662875,
      "nonce": 2197622983,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "0000000000000000000129051d90e404c3121db0c0b8b279fe07659b0e5049cc",
      "height": 817964,
      "version": 537034752,
      "timestamp": 1700663537,
      "tx_count": 5106,
      "size": 1881719,
      "weight": 3993044,
      "merkle_root": "0e4d450fafbf93a1ae97d2a6db99dae8090cbde7a0a67358a598aefb48f4eedc",
      "previousblockhash": "000000000000000000021735488c7af48086f46ab60af0c7e4dbe7fbcca8c524",
      "mediantime": 1700662760,
      "nonce": 2410092872,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "000000000000000000021735488c7af48086f46ab60af0c7e4dbe7fbcca8c524",
      "height": 817963,
      "version": 553771008,
      "timestamp": 1700663399,
      "tx_count": 5407,
      "size": 1871817,
      "weight": 3993084,
      "merkle_root": "ed6df43fcdc28164b181d8a5e0563a60792454677ac2aed903194cdfcee5f74d",
      "previousblockhash": "00000000000000000000fafed3a6c88c410d52ad72aadba2a3289f3b33caf911",
      "mediantime": 1700662610,
      "nonce": 3585243226,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "00000000000000000000fafed3a6c88c410d52ad72aadba2a3289f3b33caf911",
      "height": 817962,
      "version": 721895424,
      "timestamp": 1700663267,
      "tx_count": 5436,
      "size": 1848998,
      "weight": 3993593,
      "merkle_root": "74a47e21bc53db5f0032b0ded76a7fda4a373c4f323338d5a0eecf85dc3a684b",
      "previousblockhash": "000000000000000000044ebad84b2529cf8c0fa1b6edac838d6b4266e4dd8300",
      "mediantime": 1700661495,
      "nonce": 84794820,
      "bits": 386161170,
      "difficulty": 64678587803496
    },
    {
      "id": "000000000000000000044ebad84b2529cf8c0fa1b6edac838d6b4266e4dd8300",
      "height": 817961,
      "version": 841957376,
      "timestamp": 1700663138,
      "tx_count": 4534,
      "size": 1752734,
      "weight": 3993044,
      "merkle_root": "afadcebc3d8313c4af074efa17fdeb100a2095e01cb3148fcd8fd6d270d629a2",
      "previousblockhash": "000000000000000000033d19e7cacbc0ea0901934f9d41067f59cb0d79939f59",
      "mediantime": 1700661115,
      "nonce": 1425020306,
      "bits": 386161170,
      "difficulty": 64678587803496
    }
  ]
}

```

</details>

### Block Tip Height <a href="#user-content-get-blockstipheight" id="user-content-get-blockstipheight"></a>

Returns the height of the last block.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_blocks:tip:height",
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

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": 817988
}
```

</details>

### Block Tip Hash <a href="#user-content-get-blockstiphash" id="user-content-get-blockstiphash"></a>

Returns the hash of the last block.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_blocks:tip:hash",
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

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "00000000000000000002738dc6915fbc6757dbed33a0faca0aa6402e2e6a3355"
}
```

</details>

## Mempool <a href="#user-content-mempool" id="user-content-mempool"></a>

### Mempool Stats <a href="#user-content-get-mempool" id="user-content-get-mempool"></a>

Get mempool backlog statistics. Returns an object with:

* `count`: the number of transactions in the mempool
* `vsize`: the total size of mempool transactions in virtual bytes
* `total_fee`: the total fee paid by mempool transactions in satoshis
*   `fee_histogram`: mempool fee-rate distribution histogram

    An array of `(feerate, vsize)` tuples, where each entry's `vsize` is the total vsize of transactions paying more than `feerate` but less than the previous entry's `feerate` (except for the first entry, which has no upper bound). This matches the format used by the Electrum RPC protocol for `mempool.get_fee_histogram`.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_mempool",
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

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "count": 29236,
    "vsize": 13878067,
    "total_fee": 487586136,
    "fee_histogram": [[53.01, 102131], [38.56, 110990], [34.12, 138976], [24.34, 112619], [3.16, 246346], [2.92, 239701], [1.1, 775272]]
  }
}
```

In this example, there are transactions weighting a total of 102,131 vbytes that are paying more than 53 sat/vB, 110,990 vbytes of transactions paying between 38 and 53 sat/vB, 138,976 vbytes paying between 34 and 38, etc.

</details>

### Mempool Tx IDs <a href="#user-content-get-mempooltxids" id="user-content-get-mempooltxids"></a>

Get the full list of txids in the mempool as an array.

The order of the txids is arbitrary and does not match bitcoind's.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_mempool:txids",
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

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    "25d04ab1d202ec1835d47779557d2a63af59431a436fdbf1a7c1e8e39f93dfbf",
    "b8e223f537d026a26b77e5b4a10701d684fe66bca578ba3783125f1ece27eb8c",
    "0bb83d3f849f80a0b3111d93ee20bef4a6180d101ed46649c95ce791b6efcf84",
    "26e4bd9816ed4bd1e38368a07092f4e2fddc88d46baa0c534b2b61ea4de903b4"
  ]
}
```

</details>

### Mempool Recent Txs <a href="#user-content-get-mempoolrecent" id="user-content-get-mempoolrecent"></a>

Get a list of the last 10 transactions to enter the mempool.

Each transaction object contains simplified overview data, with the following fields: `txid`, `fee`, `vsize` and `value`

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_mempool:recent",
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

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "txid": "8d9ab9e6e55975ca1d106f56534175665987631db729aa78ac751f7ad373ffab",
      "fee": 98560,
      "vsize": 1118,
      "value": 17212160
    },
    {
      "txid": "c9dca4203c3f9c12c4cdd7577c1ad9198a98e7138866c880aac6211a9de66858",
      "fee": 9520,
      "vsize": 143,
      "value": 65426
    },
    {
      "txid": "e59823b30f2d23f33a6df519ef51ab25a9e529505c32caca70a5791c9643a52d",
      "fee": 21840,
      "vsize": 181,
      "value": 24000
    },
    {
      "txid": "6c4197d351586c5a4f2f4e1ff3b962db96f61c8456c7c36709640e1fd65e0c82",
      "fee": 16850,
      "vsize": 226,
      "value": 437850
    },
    {
      "txid": "e12c115b438d2af93440fcdc435334af1d1c4d362a3fb3874fe43e11f4a705c4",
      "fee": 18701,
      "vsize": 143,
      "value": 549600
    },
    {
      "txid": "653426670f82a60a2b34535ded191ff6f7b19d9148cacec66d740282feca081d",
      "fee": 81292,
      "vsize": 296,
      "value": 81882853
    },
    {
      "txid": "6b6e20cd2ac09ec8d62699cefe1f47ddc88f3ea00b9bb435d9292da9c3a4f390",
      "fee": 21304,
      "vsize": 141,
      "value": 2650154
    },
    {
      "txid": "0f3e825eb60f86c15d5f0ec1efbe155fce9dbde1a64c12a6e9ed52336c51ffde",
      "fee": 32225,
      "vsize": 140,
      "value": 29597400
    },
    {
      "txid": "1c74adcd1c6c46162b8b6b79d24d3df532f2b6f552ae9e496d741df54d173eba",
      "fee": 12939,
      "vsize": 110,
      "value": 272497
    },
    {
      "txid": "124b9f4fa5ea77b608511b6ddd125774744545dfcdd9cabe31c2b2dea2facd09",
      "fee": 18407,
      "vsize": 171,
      "value": 6650486
    }
  ]
}

```

</details>

## Fees <a href="#user-content-fee-estimates" id="user-content-fee-estimates"></a>

### Fee Estimates <a href="#user-content-get-fee-estimates" id="user-content-get-fee-estimates"></a>

Get an object where the key is the confirmation target (in number of blocks) and the value is the estimated fee rate (in sat/vB).

The available confirmation targets are 1-25, 144, 504 and 1008 blocks.

{% tabs %}
{% tab title="curl" %}
```sh
curl https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_fee-estimates",
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

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "1": 230.824,
    "2": 230.824,
    "3": 230.824,
    "4": 230.824,
    "5": 230.824,
    "6": 230.824,
    "7": 230.824,
    "8": 230.824,
    "9": 230.824,
    "10": 230.824,
    "11": 230.824,
    "12": 230.824,
    "13": 199.236,
    "14": 199.236,
    "15": 199.236,
    "16": 199.236,
    "17": 199.236,
    "18": 199.236,
    "19": 199.236,
    "20": 199.236,
    "21": 199.236,
    "22": 199.236,
    "23": 199.236,
    "24": 199.236,
    "25": 134.818,
    "144": 55.785000000000004,
    "504": 50.757000000000005,
    "1008": 50.757000000000005
  }
}
```

</details>
