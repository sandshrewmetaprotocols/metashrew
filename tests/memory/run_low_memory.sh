#!/bin/bash
# Run memory stress test in LOW memory environment (512MB)
# This should FAIL due to host memory exhaustion

set -e

echo "🔬 Building Docker image..."
docker build -f tests/memory/Dockerfile -t metashrew-memory-test .

echo ""
echo "🚀 Running test in LOW memory container (512MB)..."
echo "Expected: WASM completes successfully, stores ERROR result"
echo "  (allocations fail due to low memory)"
echo ""

docker run --rm \
    --memory=512m \
    --memory-swap=512m \
    metashrew-memory-test

echo ""
echo "✅ Test completed (check output for results)"
