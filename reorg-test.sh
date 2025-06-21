#!/bin/bash

# Improved Reorg Test Script for Metashrew
# This script creates a reorg by invalidating the last 4 blocks

set -e

# Configuration
BITCOIN_RPC_URL="http://localhost:8332"
METASHREW_RPC_URL="http://localhost:8080"
BITCOIN_CLI="bitcoin-cli"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "${BLUE}[$(date '+%Y-%m-%d %H:%M:%S')]${NC} $1"
}

success() {
    echo -e "${GREEN}✅ $1${NC}"
}

error() {
    echo -e "${RED}❌ $1${NC}"
}

warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

# Function to make RPC calls to Bitcoin Core
bitcoin_rpc() {
    local method="$1"
    shift
    local params=""
    
    if [ $# -gt 0 ]; then
        params=$(printf '"%s",' "$@")
        params="[${params%,}]"
    else
        params="[]"
    fi
    
    curl -s -X POST \
        -H "Content-Type: application/json" \
        -d "{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params}" \
        "$BITCOIN_RPC_URL" | jq -r '.result'
}

# Function to make RPC calls to Metashrew
metashrew_rpc() {
    local method="$1"
    shift
    local params=""
    
    if [ $# -gt 0 ]; then
        if [[ "$1" =~ ^[0-9]+$ ]]; then
            # Numeric parameter
            params=$(printf '%s,' "$@")
            params="[${params%,}]"
        else
            # String parameter
            params=$(printf '"%s",' "$@")
            params="[${params%,}]"
        fi
    else
        params="[]"
    fi
    
    curl -s -X POST \
        -H "Content-Type: application/json" \
        -d "{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params}" \
        "$METASHREW_RPC_URL" | jq -r '.result'
}

# Function to wait for Metashrew to sync
wait_for_sync() {
    local target_height="$1"
    local max_wait="${2:-60}"
    local start_time=$(date +%s)
    
    log "Waiting for Metashrew to sync to height $target_height..."
    
    while true; do
        local current_time=$(date +%s)
        local elapsed=$((current_time - start_time))
        
        if [ $elapsed -gt $max_wait ]; then
            error "Timeout waiting for Metashrew to sync to height $target_height"
            return 1
        fi
        
        local current_height
        current_height=$(metashrew_rpc "metashrew_height" 2>/dev/null || echo "0")
        
        if [ "$current_height" != "null" ] && [ "$current_height" -ge "$target_height" ]; then
            success "Metashrew synced to height $current_height"
            return 0
        fi
        
        log "Current height: $current_height, target: $target_height (${elapsed}s elapsed)"
        sleep 2
    done
}

# Main test function
run_reorg_test() {
    log "🚀 Starting improved reorg test..."
    log "📝 This test invalidates the last 4 blocks to ensure reorg detection works properly"
    
    # Get current Bitcoin tip
    local bitcoin_tip
    bitcoin_tip=$(bitcoin_rpc "getblockcount")
    log "📊 Bitcoin tip: $bitcoin_tip"
    
    # Wait for Metashrew to sync
    wait_for_sync "$bitcoin_tip" 60
    
    # Store original block hashes for the last 4 blocks
    declare -A original_blocks
    log "💾 Storing original block hashes..."
    
    for i in {0..3}; do
        local height=$((bitcoin_tip - i))
        if [ $height -ge 0 ]; then
            local block_hash
            block_hash=$(bitcoin_rpc "getblockhash" "$height")
            original_blocks[$height]="$block_hash"
            log "   Block $height: ${block_hash:0:16}..."
        fi
    done
    
    # Create reorg by invalidating the last 4 blocks
    local invalidate_height=$((bitcoin_tip - 3))
    log "🔄 Creating reorg by invalidating block at height $invalidate_height"
    
    local block_to_invalidate="${original_blocks[$invalidate_height]}"
    if [ -z "$block_to_invalidate" ]; then
        error "No block hash stored for height $invalidate_height"
        return 1
    fi
    
    log "❌ Invalidating block: ${block_to_invalidate:0:16}..."
    bitcoin_rpc "invalidateblock" "$block_to_invalidate" >/dev/null
    
    # Generate new blocks to replace the invalidated ones
    log "⛏️  Generating 4 replacement blocks..."
    bitcoin_rpc "generatetoaddress" "4" "bcrt1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh" >/dev/null
    
    local new_tip
    new_tip=$(bitcoin_rpc "getblockcount")
    log "📈 New Bitcoin tip after reorg: $new_tip"
    
    # Verify that block hashes have changed
    log "🔍 Verifying block hash changes..."
    for i in {0..3}; do
        local height=$((invalidate_height + i))
        local new_hash
        new_hash=$(bitcoin_rpc "getblockhash" "$height")
        local original_hash="${original_blocks[$height]}"
        
        if [ -n "$original_hash" ] && [ "$new_hash" != "$original_hash" ]; then
            log "   Block $height hash changed:"
            log "     Old: ${original_hash:0:16}..."
            log "     New: ${new_hash:0:16}..."
        else
            warning "Block $height hash unchanged (might be expected for some blocks)"
        fi
    done
    
    # Wait for Metashrew to detect and handle the reorg
    log "⏳ Waiting for Metashrew to detect reorg..."
    sleep 10
    
    local reorg_detected=false
    local attempts=0
    local max_attempts=30
    
    while [ "$reorg_detected" = false ] && [ $attempts -lt $max_attempts ]; do
        attempts=$((attempts + 1))
        
        # Get current block hash from Bitcoin
        local current_bitcoin_hash
        current_bitcoin_hash=$(bitcoin_rpc "getblockhash" "$invalidate_height")
        
        # Get block hash from Metashrew
        local metashrew_hash
        metashrew_hash=$(metashrew_rpc "metashrew_getblockhash" "$invalidate_height" 2>/dev/null || echo "null")
        
        if [ "$metashrew_hash" = "null" ]; then
            log "   Attempt $attempts: Metashrew doesn't have block $invalidate_height yet"
        elif [ "$current_bitcoin_hash" != "${metashrew_hash#0x}" ]; then
            log "   Attempt $attempts: Reorg still being processed..."
            log "     Bitcoin:   ${current_bitcoin_hash:0:16}..."
            log "     Metashrew: ${metashrew_hash:0:18}..."
        else
            success "Reorg handled! Block $invalidate_height hashes now match"
            log "   Hash: ${current_bitcoin_hash:0:16}..."
            reorg_detected=true
        fi
        
        if [ "$reorg_detected" = false ]; then
            sleep 2
        fi
    done
    
    # Verify state roots are valid (non-zero)
    log "🌳 Verifying state root changes..."
    local state_roots_valid=true
    
    for i in {0..3}; do
        local height=$((invalidate_height + i))
        local state_root
        state_root=$(metashrew_rpc "metashrew_stateroot" "$height" 2>/dev/null || echo "null")
        
        if [ "$state_root" = "null" ]; then
            warning "Could not get state root for height $height"
        elif [ "$state_root" = "0x0000000000000000000000000000000000000000000000000000000000000000" ]; then
            error "State root at height $height is all zeros!"
            state_roots_valid=false
        else
            log "   State root at height $height: ${state_root:0:18}..."
        fi
    done
    
    # Cleanup: reconsider the invalidated blocks
    log "🧹 Cleaning up..."
    for height in "${!original_blocks[@]}"; do
        local block_hash="${original_blocks[$height]}"
        bitcoin_rpc "reconsiderblock" "$block_hash" >/dev/null 2>&1 || true
        log "   Reconsidered block $height: ${block_hash:0:16}..."
    done
    
    # Results
    echo
    log "📊 Test Results:"
    if [ "$reorg_detected" = true ]; then
        success "   Reorg Detection: PASS"
    else
        error "   Reorg Detection: FAIL"
    fi
    
    if [ "$state_roots_valid" = true ]; then
        success "   State Roots:     PASS"
    else
        error "   State Roots:     FAIL"
    fi
    
    if [ "$reorg_detected" = true ] && [ "$state_roots_valid" = true ]; then
        echo
        success "🎉 Reorg test PASSED! The indexer correctly detected and handled the reorganization."
        return 0
    else
        echo
        error "❌ Reorg test FAILED! There are issues with reorg detection or state root calculation."
        return 1
    fi
}

# Check dependencies
check_dependencies() {
    if ! command -v curl >/dev/null 2>&1; then
        error "curl is required but not installed"
        return 1
    fi
    
    if ! command -v jq >/dev/null 2>&1; then
        error "jq is required but not installed"
        return 1
    fi
    
    return 0
}

# Main execution
main() {
    if ! check_dependencies; then
        error "Missing dependencies. Please install curl and jq."
        exit 1
    fi
    
    if run_reorg_test; then
        exit 0
    else
        exit 1
    fi
}

# Run the test if this script is executed directly
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    main "$@"
fi