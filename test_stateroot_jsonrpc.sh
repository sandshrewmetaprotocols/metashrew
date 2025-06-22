#!/bin/bash

echo "Testing metashrew_stateroot JSON-RPC endpoint..."

# Test metashrew_stateroot with "latest"
echo "Calling metashrew_stateroot with 'latest'..."
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{
    "id": 1,
    "jsonrpc": "2.0",
    "method": "metashrew_stateroot",
    "params": ["latest"]
  }' \
  --max-time 5

echo -e "\n\nCalling metashrew_stateroot with height 0..."
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "metashrew_stateroot",
    "params": [0]
  }' \
  --max-time 5

echo -e "\n\nCalling metashrew_height to check current height..."
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{
    "id": 3,
    "jsonrpc": "2.0",
    "method": "metashrew_height",
    "params": []
  }' \
  --max-time 5

echo -e "\n\nTesting completed."