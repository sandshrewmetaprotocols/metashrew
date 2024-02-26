# Sandshrew RPC

## Overview

This section is your gateway to understanding the wealth of RPC methods offered by Sandshrew. Sandshrew's RPC Indexer empowers developers to seamlessly integrate Bitcoin and ordinals into their applications, providing all the essential tools to develop, manage ordinals, create inscription services, and initiate Bitcoin transactions.

Sandshrew is designed around three distinct RPC services, each tailored to specific needs:

1. [**Ordinals Service**](ordinals-rpc.md)**:** This service equips you with the capabilities to create inscriptions and engage with your ordinals, offering a comprehensive solution for managing and interacting with these unique identifiers.
2. [**Esplora Service**](esplora-rpc.md)**:** Easily access transaction histories, retrieve block information, and check address information with this feature-rich service.
3. [**Full Bitcoin Node Service**](bitcoin-core-rpc.md)**:** Gain direct access to a full Bitcoin node, providing developers with an extensive array of data and functionality for in-depth blockchain analysis and integration.

To dive deeper into the Sandshrew ecosystem and learn how to set up and operate a Sandshrew indexer, consult the [Sandshrew Indexer Guide](sandshrew-indexer.md) for detailed installation and operational instructions.&#x20;

## API Keys

Sandshrew developers need to obtain a Sandshrew API Key to interact with the Sandshrew RPC Services. Refer to the [Getting Started guide](../welcome/getting-started.md) for more information on API keys.

## Sandshrew Transactions

All Sandshrew RPC Transactions have a common structure. For example, here is a curl query that calls the `getchaintxstats` method:

```
curl -s https://signet.sandshrew.io/v1/<developer key> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0", 
    "id": 1, 
    "method": "btc_getchaintxstats",
    "params": [2016]
}'
```

The parameters are defined below.

### jsonrpc

JSON-RPC is a remote procedure call protocol encoded in JSON (JavaScript Object Notation). The "JSON-RPC version" refers to the specific version of this protocol.

The Sandshrew RPC service uses version `"2.0"`

### id

In a JSON-RPC call, the `id` field enables you to associate a specific request with its corresponding response. JSON-RPC, being a stateless protocol, needs a way to match responses to requests, especially when multiple requests are sent over the same connection. Here's how the `id` field is used:

* Unique Identifier: The `id` in a JSON-RPC call is a unique identifier for that specific request. It can be a string, a number, or `null`. The client that generates the request assigns this `id`.
* Correlating Requests and Responses: When the server processes the request and sends back a response, the response will include the same `id` value as the corresponding request. This allows the client to match the response with the original request, especially important when multiple requests are made in quick succession or concurrently.
* Handling Asynchronous Operations: In asynchronous operations where responses might be received in a different order than requests were sent, the `id` field is essential to determine which response corresponds to which request.
* Differentiating Between Notifications and Requests: If a request is made where a response is not expected (known as a notification in JSON-RPC 2.0), the `id` is set to `null`. This signals to the server that no response is expected for this particular message.
* Error Handling: If there’s an error in processing the request, the server sends back an error response which also includes the `id` of the request. This helps in identifying which request led to the error.

### RPC method

The `"method"` can be any method supported by the [Bitcoin Core](bitcoin-core-rpc.md), [Esplora](esplora-rpc.md), or [Ordinals](ordinals-rpc.md) RPC services.

### RPC parameters

The `"params"` is an array of one or more parameters required by the method. The required parameters are defined in the [Sandshrew RPC](sandshrew-rpc.md) documentation.
