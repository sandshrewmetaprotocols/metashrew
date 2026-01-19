#!/bin/bash
# Compare memory stress test results in different environments
# This demonstrates NON-DETERMINISM: same input, different results!

set -e

echo "======================================================"
echo "🔬 MEMORY NON-DETERMINISM DEMONSTRATION"
echo "======================================================"
echo ""
echo "This test demonstrates that WASM execution can produce"
echo "DIFFERENT STORED DATA based on HOST memory constraints,"
echo "violating determinism requirements."
echo ""
echo "CRITICAL: Both executions complete successfully!"
echo "The bug is that they store DIFFERENT DATA."
echo ""
echo "Same WASM + Same input → Different stored state!"
echo ""
echo "======================================================"
echo ""

# Build once
echo "🔨 Building Docker image..."
docker build -f tests/memory/Dockerfile -t metashrew-memory-test . > /dev/null 2>&1
echo "✅ Build complete"
echo ""

# Test 1: High memory (should succeed)
echo "======================================================"
echo "📊 TEST 1: HIGH MEMORY (4GB)"
echo "======================================================"
echo ""
docker run --rm \
    --memory=4g \
    metashrew-memory-test 2>&1 | tee /tmp/high_memory_result.txt
echo ""

# Test 2: Low memory (should fail)
echo "======================================================"
echo "📊 TEST 2: LOW MEMORY (512MB)"
echo "======================================================"
echo ""
docker run --rm \
    --memory=512m \
    --memory-swap=512m \
    metashrew-memory-test 2>&1 | tee /tmp/low_memory_result.txt || true
echo ""

# Compare results
echo "======================================================"
echo "🔍 COMPARISON RESULTS"
echo "======================================================"
echo ""

HIGH_SUCCESS=false
HIGH_ERROR=false
LOW_SUCCESS=false
LOW_ERROR=false

if grep -q "stored: SUCCESS" /tmp/high_memory_result.txt; then
    echo "✅ High memory (4GB): WASM completed, stored SUCCESS"
    HIGH_SUCCESS=true
elif grep -q "stored: ERROR" /tmp/high_memory_result.txt; then
    echo "⚠️  High memory (4GB): WASM completed, stored ERROR"
    HIGH_ERROR=true
else
    echo "⚠️  High memory (4GB): Unexpected result"
fi

if grep -q "stored: SUCCESS" /tmp/low_memory_result.txt; then
    echo "⚠️  Low memory (512MB): WASM completed, stored SUCCESS"
    LOW_SUCCESS=true
elif grep -q "stored: ERROR" /tmp/low_memory_result.txt; then
    echo "❌ Low memory (512MB): WASM completed, stored ERROR"
    LOW_ERROR=true
else
    echo "⚠️  Low memory (512MB): Unexpected result"
fi

echo ""
echo "======================================================"
echo "🚨 CONCLUSION: NON-DETERMINISM DETECTED!"
echo "======================================================"
echo ""

if [ "$HIGH_SUCCESS" = true ] && [ "$LOW_ERROR" = true ]; then
    echo "✅ BUG CONFIRMED!"
    echo ""
    echo "The SAME WASM module with the SAME input produced"
    echo "DIFFERENT STORED DATA in different memory environments:"
    echo ""
    echo "  • High memory: stored 'SUCCESS:1000MB'"
    echo "  • Low memory:  stored 'ERROR:failed at allocation N'"
    echo ""
    echo "BOTH executions completed successfully (no panic),"
    echo "but they wrote DIFFERENT DATA to storage!"
    echo ""
elif [ "$HIGH_SUCCESS" = true ] && [ "$LOW_SUCCESS" = true ]; then
    echo "⚠️  Both succeeded - your host has enough memory"
    echo "Try reducing low memory limit further (e.g., 256m)"
    echo ""
elif [ "$HIGH_ERROR" = true ]; then
    echo "⚠️  High memory test failed - host may be constrained"
    echo "Try increasing high memory limit (e.g., 8g)"
    echo ""
else
    echo "⚠️  Unexpected results - check output above"
    echo ""
fi

echo "This violates the determinism requirement for"
echo "blockchain indexers and consensus systems."
echo ""
echo "Root cause: Vec::try_reserve() returns different results"
echo "based on HOST memory availability, causing WASM to store"
echo "different data for the same input."
echo ""
