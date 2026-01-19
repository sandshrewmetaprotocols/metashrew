#!/bin/bash
# Run memory stress test in HIGH memory environment (4GB)
# This should SUCCEED with sufficient host memory

set -e

echo "🔬 Building Docker image..."
docker build -f tests/memory/Dockerfile -t metashrew-memory-test .

echo ""
echo "🚀 Running test in HIGH memory container (4GB)..."
echo "Expected: WASM completes successfully, stores SUCCESS result"
echo "  (allocations succeed with sufficient memory)"
echo ""

docker run --rm \
    --memory=4g \
    metashrew-memory-test

echo ""
echo "✅ Test completed (check output for results)"
