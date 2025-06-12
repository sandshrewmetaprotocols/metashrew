# JSON-RPC Interface

Metashrew provides a comprehensive JSON-RPC API for querying indexed data. This document explains the available methods, request/response format, view functions, and historical state queries.

## Overview

The JSON-RPC interface follows the [JSON-RPC 2.0 specification](https://www.jsonrpc.org/specification) and is exposed via HTTP. It allows clients to query indexed data, execute view functions, and retrieve historical state.

## Request/Response Format

### Request Format

JSON-RPC requests follow this format:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "method_name",
  "params": [param1, param2, ...]
}
```

Where:
- `jsonrpc`: Must be "2.0"
- `id`: A unique identifier for the request
- `method`: The name of the method to call
- `params`: An array of parameters for the method

### Response Format

Successful responses follow this format:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "result_data"
}
```

Error responses follow this format:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32000,
    "message": "Error message",
    "data": "Additional error data (optional)"
  }
}
```

## Available Methods

Metashrew provides the following JSON-RPC methods:

### 1. `metashrew_view`

Executes a view function from the WASM module.

**Parameters**:
1. `view_name` (string): The name of the view function to execute
2. `input_data` (hex string): Input data for the view function
3. `height` (number or "latest"): Block height for historical queries

**Example Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_view",
  "params": ["get_address_balance", "0x1a2b3c4d", "latest"]
}
```

**Example Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0x0000000000989680"
}
```

### 2. `metashrew_preview`

Executes a view function with a custom block without persisting changes.

**Parameters**:
1. `block_data` (hex string): Block data to process
2. `view_name` (string): The name of the view function to execute
3. `input_data` (hex string): Input data for the view function
4. `height` (number or "latest"): Block height for historical queries

**Example Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_preview",
  "params": ["0x...", "get_address_balance", "0x1a2b3c4d", 500000]
}
```

**Example Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0x0000000000989680"
}
```

### 3. `metashrew_height`

Returns the current indexed block height.

**Parameters**: None

**Example Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_height",
  "params": []
}
```

**Example Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "500000"
}
```

### 4. `metashrew_getblockhash`

Returns the block hash for a given height.

**Parameters**:
1. `height` (number): Block height

**Example Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_getblockhash",
  "params": [500000]
}
```

**Example Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0x00000000000000000024fb37364cbf81fd49cc2d51c09c75c35433c3a1945d04"
}
```

### 5. `metashrew_stateroot`

Returns the state root for a given height.

**Parameters**:
1. `height` (number or "latest"): Block height

**Example Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_stateroot",
  "params": [500000]
}
```

**Example Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0x7b226b657973223a5b22746f6b656e5f737570706c79225d2c2276616c756573223a5b22307830303030303030303030393839363830225d7d"
}
```

### 6. `metashrew_query`

Queries the historical state of a key at a specific height using the Height-Indexed BST.

**Parameters**:
1. `key` (hex string): The key to query
2. `height` (number or "latest"): Block height

**Example Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_query",
  "params": ["0x616464726573733a31413242334334443a62616c616e6365", 500000]
}
```

**Example Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0x0000000000989680"
}
```

## View Functions

View functions are custom query functions defined in the WASM module. They allow for complex queries that can't be expressed as simple key-value lookups.

### Defining View Functions

View functions are defined in the WASM module as exported functions with this signature:

```rust
#[no_mangle]
pub extern "C" fn view_function_name(input_ptr: i32, input_len: i32) -> i32;
```

Where:
- `input_ptr`: Pointer to the input data in WASM memory
- `input_len`: Length of the input data
- Return value: Pointer to the result data in WASM memory

### Example View Function

Here's an example of a view function that returns the balance of an address:

```rust
#[no_mangle]
pub extern "C" fn get_address_balance(input_ptr: i32, input_len: i32) -> i32 {
    let input = unsafe { std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize) };
    let address = std::str::from_utf8(input).unwrap();
    
    let key = format!("address:{}:balance", address).into_bytes();
    let result = match get(&key).unwrap() {
        Some(balance_data) => balance_data,
        None => vec![0, 0, 0, 0, 0, 0, 0, 0], // Zero balance
    };
    
    // Allocate memory for the result
    let result_ptr = alloc(result.len());
    unsafe {
        std::ptr::copy_nonoverlapping(result.as_ptr(), result_ptr, result.len());
    }
    
    result_ptr as i32
}
```

### Calling View Functions

View functions are called using the `metashrew_view` method:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_view",
  "params": ["get_address_balance", "0x31413242334334443536", "latest"]
}
```

The input data is passed as a hex string, and the result is returned as a hex string.

## Historical State Queries

Metashrew supports querying the state at any historical block height. This is useful for analyzing how the state has evolved over time or for implementing time-locked contracts.

### Height Parameter

Most query methods accept a `height` parameter that can be either:

- A specific block height (e.g., `500000`)
- The string `"latest"` to query the latest indexed block

### Historical View Function Execution

When executing a view function with a historical height, Metashrew ensures that the function sees the state as it was at that height:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_view",
  "params": ["get_token_supply", "0x544f4b454e", 500000]
}
```

This executes the `get_token_supply` view function as if the current state was at block height 500000.

### Direct Historical State Queries

For simple key-value lookups, the `metashrew_query` method provides efficient historical state queries using the Height-Indexed BST:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_query",
  "params": ["0x746f6b656e5f737570706c79", 500000]
}
```

This returns the value of the key `token_supply` at block height 500000.

## Error Codes

Metashrew uses the following error codes in JSON-RPC responses:

| Code    | Message                  | Description                                   |
|---------|--------------------------|-----------------------------------------------|
| -32600  | Invalid Request          | The JSON sent is not a valid Request object   |
| -32601  | Method not found         | The method does not exist / is not available  |
| -32602  | Invalid params           | Invalid method parameter(s)                   |
| -32603  | Internal error           | Internal JSON-RPC error                       |
| -32000  | Server error             | Generic server error                          |
| -32001  | View function not found  | The specified view function does not exist    |
| -32002  | View function error      | Error executing the view function             |
| -32003  | Invalid height           | The specified height is invalid               |
| -32004  | Block not found          | The specified block was not found             |

## Authentication

By default, the JSON-RPC interface does not require authentication. However, it's recommended to secure the interface in production environments using one of these methods:

1. **Reverse Proxy**: Place the JSON-RPC server behind a reverse proxy with authentication
2. **Firewall Rules**: Restrict access to the JSON-RPC port
3. **VPN**: Access the JSON-RPC server through a VPN

## CORS Configuration

Cross-Origin Resource Sharing (CORS) can be configured using the `--cors` command-line option:

- `--cors "*"`: Allow all origins
- `--cors "domain1.com,domain2.com"`: Allow specific origins
- No `--cors` option: Only allow localhost

## Rate Limiting

Metashrew does not implement rate limiting at the application level. For production deployments, it's recommended to implement rate limiting at the reverse proxy or firewall level.

## WebSocket Support

Currently, Metashrew only supports HTTP for JSON-RPC. WebSocket support may be added in future versions.

## Example Client Code

### JavaScript

```javascript
async function queryMetashrew(method, params) {
  const response = await fetch('http://localhost:8080', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method,
      params,
    }),
  });
  
  const data = await response.json();
  
  if (data.error) {
    throw new Error(`JSON-RPC error: ${data.error.message}`);
  }
  
  return data.result;
}

// Example usage
async function getAddressBalance(address) {
  const addressHex = '0x' + Buffer.from(address).toString('hex');
  const balanceHex = await queryMetashrew('metashrew_view', ['get_address_balance', addressHex, 'latest']);
  
  // Convert hex string to number
  const balance = BigInt('0x' + balanceHex.slice(2));
  return balance;
}
```

### Python

```python
import requests
import json
import binascii

def query_metashrew(method, params):
    response = requests.post(
        'http://localhost:8080',
        json={
            'jsonrpc': '2.0',
            'id': 1,
            'method': method,
            'params': params,
        }
    )
    
    data = response.json()
    
    if 'error' in data:
        raise Exception(f"JSON-RPC error: {data['error']['message']}")
    
    return data['result']

# Example usage
def get_address_balance(address):
    address_hex = '0x' + binascii.hexlify(address.encode()).decode()
    balance_hex = query_metashrew('metashrew_view', ['get_address_balance', address_hex, 'latest'])
    
    # Convert hex string to number
    balance = int(balance_hex[2:], 16)
    return balance
```

## Conclusion

The JSON-RPC interface provides a flexible and powerful way to query indexed data from Metashrew. By combining view functions with historical state queries, clients can access complex data structures and analyze how the state has evolved over time.

Whether you're building a simple block explorer or a complex DeFi application, the JSON-RPC interface provides the tools you need to access and analyze blockchain data efficiently.