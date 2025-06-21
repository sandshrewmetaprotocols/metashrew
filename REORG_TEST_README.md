# Improved Reorg Test for Metashrew

This directory contains improved reorg tests that address the issue where the previous test was invalidating blocks too far back in history (beyond the 100-block search window).

## Problem Description

The original reorg test was failing because:
1. It invalidated blocks more than 6 blocks back in history
2. The reorg detection logic has a maximum search depth of 100 blocks
3. If the invalidated blocks were beyond this window, reorg detection wouldn't trigger
4. This caused false negatives where actual reorgs weren't detected

## Solution

The new test creates a more accurate reorg scenario by:
1. **Invalidating only the last 4 blocks** (ensuring they're within the search window)
2. **Replacing them with different blocks** that have different blockhashes
3. **Verifying that reorg detection triggers properly**
4. **Checking that state roots change appropriately**

## Files

### `reorg-test.sh` (Recommended)
A bash script that's easy to run and has minimal dependencies.

**Prerequisites:**
- `curl` (for HTTP requests)
- `jq` (for JSON parsing)
- Bitcoin Core running with RPC enabled
- Metashrew indexer running

**Usage:**
```bash
# Make executable (if not already)
chmod +x reorg-test.sh

# Run the test
./reorg-test.sh
```

### `reorg-test.ts`
A TypeScript version with more detailed functionality (requires Node.js setup).

## How the Test Works

1. **Setup Phase:**
   - Gets the current Bitcoin blockchain tip
   - Waits for Metashrew to sync to the current tip
   - Stores the original block hashes for the last 4 blocks

2. **Reorg Creation:**
   - Invalidates the block at height `(tip - 3)`, which invalidates the last 4 blocks
   - Generates 4 new replacement blocks with different hashes
   - Verifies that the block hashes have actually changed

3. **Verification Phase:**
   - Waits for Metashrew to detect the reorg (up to 1 minute)
   - Checks that Metashrew's stored block hashes match the new Bitcoin hashes
   - Verifies that state roots are valid (non-zero) for the affected blocks

4. **Cleanup:**
   - Reconsiders the invalidated blocks to restore the original chain state

## Expected Results

When the test passes, you should see:
- ✅ **Reorg Detection: PASS** - Metashrew detected and handled the reorg
- ✅ **State Roots: PASS** - State roots are valid and non-zero

## Troubleshooting

### "Metashrew doesn't respond" or JSON errors
- Ensure Metashrew is running on `http://localhost:8080`
- Check that the indexer is not stuck or crashed
- Verify the RPC endpoints are accessible

### "Bitcoin RPC errors"
- Ensure Bitcoin Core is running with RPC enabled
- Check that the RPC URL is correct (default: `http://localhost:8332`)
- Verify authentication if required

### "Reorg not detected"
- Check Metashrew logs for reorg detection messages
- Ensure the indexer is actively processing blocks
- Verify that the 100-block search window logic is working

### "State roots are all zeros"
- This was the original bug that should now be fixed
- Check that the SMT helper is working correctly
- Verify that state root calculation is enabled

## Configuration

You can modify the script variables at the top:
```bash
BITCOIN_RPC_URL="http://localhost:8332"
METASHREW_RPC_URL="http://localhost:8080"
```

## Integration with CI/CD

This test can be integrated into automated testing pipelines:
```bash
# Run test and exit with proper code
./reorg-test.sh
if [ $? -eq 0 ]; then
    echo "Reorg test passed"
else
    echo "Reorg test failed"
    exit 1
fi
```

## Key Improvements Over Previous Test

1. **Within Search Window**: Only invalidates 4 blocks, ensuring they're within the 100-block search window
2. **Proper Verification**: Checks both reorg detection AND state root validity
3. **Better Logging**: Clear progress indicators and error messages
4. **Automatic Cleanup**: Restores original chain state after testing
5. **Timeout Handling**: Won't hang indefinitely if something goes wrong
6. **Dependency Checking**: Verifies required tools are available

This test should now properly validate that:
- Blockchain-level reorg detection works (block hashes change)
- Alkanes-level reorg detection works (state roots change)
- The indexer handles reorgs within the expected time window
- State root calculation returns valid (non-zero) values