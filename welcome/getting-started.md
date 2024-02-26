# Getting Started

Sandshrew's RPC service is designed to be developer-friendly and easy to integrate into your projects. In this documentation, you'll find comprehensive guides, code examples, and API references to help you make the most of Sandshrew's features.

So let's dive in and unlock the full potential of Bitcoin blockchain data for your projects.

### 1. Sign up with Sandshrew[​](https://docs.infura.io/getting-started#1-sign-up-to-infura) <a href="#id-1-sign-up-to-infura" id="id-1-sign-up-to-infura"></a>

To sign up for an account on the Sandshrew website, enter your details, and select **CREATE ACCOUNT**.

To activate your account, verify your email address by clicking the link sent to your inbox.

Once verified, you’ll be taken to the Sandshrew Dashboard where you can view analytics and raise support requests.

### 2. Create an API key[​](https://docs.infura.io/getting-started#2-create-an-api-key) <a href="#id-2-create-an-api-key" id="id-2-create-an-api-key"></a>

You must receive an API key to authenticate your network requests.

To receive an API key, head over to the [Sandshrew Discord](https://discord.gg/vdBAWbMe). Once you are verified, go to the \
"Get Started Here" channel and request an api key using the bot interface. An administrator will be in touch with you to generate the api key.

### 3. Send requests[​](https://docs.infura.io/getting-started#3-send-requests) <a href="#id-3-send-requests" id="id-3-send-requests"></a>

Using your new the API key, you can now send RPC requests. The following examples interact with the Sandshrew indexer by RPC requests using HTTPS.

{% hint style="info" %}
* All requests are `POST` requests.
* Replace YOUR-API-KEY with your own unique API key.
{% endhint %}

You can use a tool such as the [Client Uniform Resource Locator (curl)](https://docs.infura.io/learn/curl) or [Postman](https://www.postman.com/downloads/) to make requests. We recommend using Postman if you're a Windows user.

#### 3.1 Get the current block number[​](https://docs.infura.io/getting-started#31-get-the-current-block-number) <a href="#id-31-get-the-current-block-number" id="id-31-get-the-current-block-number"></a>

To retrieve the current block number:

```
curl https://signet.sandshrew.io/v1/YOUR-API-KEY \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "esplora_blocks:tip:height",
    "params": []
}'
```

You'll receive a response similar to:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "00000000000000000002738dc6915fbc6757dbed33a0faca0aa6402e2e6a3355"
}
```

#### 3.2 View the UTXOs for a Bitcoin address <a href="#id-32-view-the-ether-balance-of-a-specified-contract" id="id-32-view-the-ether-balance-of-a-specified-contract"></a>

To get the list of unspent transaction outputs associated with the address.

{% tabs %}
{% tab title="curl" %}
```sh
curl -s -XPOST https://signet.sandshrew.io/v1/YOUR-API-KEY \
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

You'll receive a response similar to:

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
    }
  ]
}
```
