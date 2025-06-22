#!/bin/bash

echo "Testing metashrew_height JSON-RPC endpoint..."

# Test metashrew_height
echo "Calling metashrew_height..."
curl -X POST http://localhost:8096 \
  -H "Content-Type: application/json" \
  -d '{
    "id": 1,
    "jsonrpc": "2.0",
    "method": "metashrew_height",
    "params": []
  }' \
  --max-time 5

echo -e "\n\nTesting completed." harness because I am bot convinced we are saving anything to the databaae whatsoever
