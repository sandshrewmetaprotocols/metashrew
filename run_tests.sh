#!/bin/bash
#
# Comprehensive test runner for Metashrew
#
# This script runs all consolidated tests with proper configuration
# and provides detailed reporting on test coverage and results.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
RUST_LOG=${RUST_LOG:-info}
RUST_BACKTRACE=${RUST_BACKTRACE:-1}

echo -e "${BLUE}=== Metashrew Comprehensive Test Suite ===${NC}"
echo "Starting comprehensive test execution..."
echo

# Function to run a test category
run_test_category() {
    local category=$1
    local description=$2
    local filter=$3
    
    echo -e "${YELLOW}Running $description...${NC}"
    
    if RUST_LOG=$RUST_LOG RUST_BACKTRACE=$RUST_BACKTRACE cargo test $filter -- --nocapture; then
        echo -e "${GREEN}✅ $description passed${NC}"
        return 0
    else
        echo -e "${RED}❌ $description failed${NC}"
        return 1
    fi
}

# Ensure WASM module is built
echo -e "${BLUE}Building WASM dependencies...${NC}"
if ! cargo build --release --target wasm32-unknown-unknown -p metashrew-minimal; then
    echo -e "${RED}❌ Failed to build WASM module${NC}"
    exit 1
fi
echo -e "${GREEN}✅ WASM module built successfully${NC}"
echo

# Initialize test results tracking
TOTAL_CATEGORIES=0
PASSED_CATEGORIES=0
FAILED_CATEGORIES=()

# Core functionality tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "core" "Core Functionality Tests" "core_functionality_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Core Functionality")
fi
echo

# Integration tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "integration" "Integration Tests" "integration_tests"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Integration")
fi
echo

# LRU Cache E2E tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "lru_cache_e2e" "LRU Cache E2E Tests" "lru_cache_e2e_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("LRU Cache E2E")
fi
echo

# View Pool E2E tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "view_pool_e2e" "View Pool E2E Tests" "view_pool_e2e_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("View Pool E2E")
fi
echo

# Reorg focused tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "reorg" "Reorganization Tests" "reorg_focused_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Reorganization")
fi
echo

# Block retry tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "retry" "Block Retry Tests" "block_retry_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Block Retry")
fi
echo

# Comprehensive E2E tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "comprehensive_e2e" "Comprehensive E2E Tests" "comprehensive_e2e_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Comprehensive E2E")
fi
echo

# Preview E2E tests
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "preview" "Preview E2E Tests" "preview_e2e_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Preview E2E")
fi
echo

# Long-running tests (optional, can be skipped with SKIP_LONG_TESTS=1)
if [[ "${SKIP_LONG_TESTS}" != "1" ]]; then
    echo -e "${YELLOW}Running long-running tests (set SKIP_LONG_TESTS=1 to skip)...${NC}"
    TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
    if run_test_category "long_running" "Long-Running Indexer Tests" "long_running_indexer_test"; then
        PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
    else
        FAILED_CATEGORIES+=("Long-Running Indexer")
    fi
    echo
else
    echo -e "${YELLOW}Skipping long-running tests (SKIP_LONG_TESTS=1)${NC}"
    echo
fi

# Legacy LRU cache tests (for compatibility)
TOTAL_CATEGORIES=$((TOTAL_CATEGORIES + 1))
if run_test_category "lru_cache_legacy" "Legacy LRU Cache Tests" "lru_cache_test"; then
    PASSED_CATEGORIES=$((PASSED_CATEGORIES + 1))
else
    FAILED_CATEGORIES+=("Legacy LRU Cache")
fi
echo

# Summary
echo -e "${BLUE}=== Test Summary ===${NC}"
echo "Total test categories: $TOTAL_CATEGORIES"
echo -e "Passed: ${GREEN}$PASSED_CATEGORIES${NC}"
echo -e "Failed: ${RED}$((TOTAL_CATEGORIES - PASSED_CATEGORIES))${NC}"

if [[ ${#FAILED_CATEGORIES[@]} -gt 0 ]]; then
    echo -e "${RED}Failed categories:${NC}"
    for category in "${FAILED_CATEGORIES[@]}"; do
        echo -e "  ${RED}- $category${NC}"
    done
    echo
    echo -e "${RED}❌ Some tests failed. Please check the output above for details.${NC}"
    exit 1
else
    echo
    echo -e "${GREEN}🎉 All tests passed successfully!${NC}"
    echo
    echo -e "${BLUE}Test Coverage Summary:${NC}"
    echo "✅ Core functionality (SMT, key utils, memory store)"
    echo "✅ LRU cache behavior and integration"
    echo "✅ View pool functionality and performance"
    echo "✅ Chain reorganization handling"
    echo "✅ Block retry mechanisms"
    echo "✅ End-to-end indexing workflows"
    echo "✅ Preview functionality"
    if [[ "${SKIP_LONG_TESTS}" != "1" ]]; then
        echo "✅ Long-running indexer stability"
    fi
    echo "✅ Legacy compatibility"
    echo
    echo -e "${GREEN}The Metashrew indexer framework is ready for production use!${NC}"
fi