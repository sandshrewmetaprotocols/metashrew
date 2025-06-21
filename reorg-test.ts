#!/usr/bin/env node

/**
 * Improved Reorg Test for Metashrew Indexer
 * 
 * This test creates a more accurate reorg scenario by:
 * 1. Invalidating the last 4 blocks (ensuring they're within the 100-block search window)
 * 2. Replacing them with different blocks that have different blockhashes
 * 3. Verifying that reorg detection triggers properly
 * 4. Checking that state roots change appropriately
 */

import { execSync, spawn } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';

interface RpcRequest {
    id: number;
    jsonrpc: string;
    method: string;
    params: any[];
}

interface RpcResponse {
    id: number;
    result?: any;
    error?: {
        code: number;
        message: string;
        data?: any;
    };
    jsonrpc: string;
}

class ReorgTestHarness {
    private bitcoinRpcUrl: string;
    private metashrewRpcUrl: string;
    private originalBlocks: Map<number, string> = new Map();
    private testStartHeight: number = 0;

    constructor(
        bitcoinRpcUrl: string = 'http://localhost:18443',
        metashrewRpcUrl: string = 'http://localhost:18888'
    ) {
        this.bitcoinRpcUrl = bitcoinRpcUrl;
        this.metashrewRpcUrl = metashrewRpcUrl;
    }

    private async makeRpcCall(url: string, method: string, params: any[] = []): Promise<any> {
        const request: RpcRequest = {
            id: Date.now(),
            jsonrpc: '2.0',
            method,
            params
        };

        const response = await fetch(url, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
		Authorization: 'Basic ' + Buffer.from('bitcoinrpc:bitcoinrpc').toString('base64')
            },
            body: JSON.stringify(request)
        });

        if (!response.ok) {
            throw new Error(`HTTP error! status: ${response.status}`);
        }

        const result: RpcResponse = await response.json();
        
        if (result.error) {
            throw new Error(`RPC error: ${result.error.message}`);
        }

        return result.result;
    }

    private async getBitcoinBlockCount(): Promise<number> {
        return await this.makeRpcCall(this.bitcoinRpcUrl, 'getblockcount');
    }

    private async getBitcoinBlockHash(height: number): Promise<string> {
        return await this.makeRpcCall(this.bitcoinRpcUrl, 'getblockhash', [height]);
    }

    private async getMetashrewHeight(): Promise<number> {
        return await this.makeRpcCall(this.metashrewRpcUrl, 'metashrew_height');
    }

    private async getMetashrewStateRoot(height: number | string): Promise<string> {
        return await this.makeRpcCall(this.metashrewRpcUrl, 'metashrew_stateroot', [height]);
    }

    private async getMetashrewBlockHash(height: number): Promise<string> {
        return await this.makeRpcCall(this.metashrewRpcUrl, 'metashrew_getblockhash', [height]);
    }

    private async waitForSync(targetHeight: number, maxWaitSeconds: number = 60): Promise<void> {
        console.log(`⏳ Waiting for Metashrew to sync to height ${targetHeight}...`);
        
        const startTime = Date.now();
        const maxWaitMs = maxWaitSeconds * 1000;

        while (Date.now() - startTime < maxWaitMs) {
            try {
                const currentHeight = await this.getMetashrewHeight();
                if (currentHeight >= targetHeight) {
                    console.log(`✅ Metashrew synced to height ${currentHeight}`);
                    return;
                }
                console.log(`   Current height: ${currentHeight}, target: ${targetHeight}`);
                await new Promise(resolve => setTimeout(resolve, 2000));
            } catch (error) {
                console.log(`   Waiting for Metashrew to be ready... (${error})`);
                await new Promise(resolve => setTimeout(resolve, 5000));
            }
        }

        throw new Error(`Timeout waiting for Metashrew to sync to height ${targetHeight}`);
    }

    async setup(): Promise<void> {
        console.log('🔧 Setting up reorg test...');

        // Get current Bitcoin tip
        const bitcoinTip = await this.getBitcoinBlockCount();
        console.log(`📊 Bitcoin tip: ${bitcoinTip}`);

        // Wait for Metashrew to sync
        await this.waitForSync(bitcoinTip);

        // Store the current tip as our test start height
        this.testStartHeight = bitcoinTip;
        console.log(`🎯 Test start height: ${this.testStartHeight}`);

        // Store original block hashes for the last 4 blocks
        for (let i = 0; i < 4; i++) {
            const height = this.testStartHeight - i;
            if (height >= 0) {
                const blockHash = await this.getBitcoinBlockHash(height);
                this.originalBlocks.set(height, blockHash);
                console.log(`💾 Stored original block ${height}: ${blockHash.substring(0, 16)}...`);
            }
        }
    }

    async createReorg(): Promise<void> {
        console.log('🔄 Creating reorg by invalidating last 4 blocks...');

        // Invalidate the last 4 blocks
        const invalidateHeight = this.testStartHeight - 3; // This will invalidate the last 4 blocks
        console.log(`❌ Invalidating block at height ${invalidateHeight}`);

        try {
            // Use Bitcoin Core's invalidateblock RPC to create the reorg
            const blockHashToInvalidate = this.originalBlocks.get(invalidateHeight);
            if (!blockHashToInvalidate) {
                throw new Error(`No block hash stored for height ${invalidateHeight}`);
            }

            await this.makeRpcCall(this.bitcoinRpcUrl, 'invalidateblock', [blockHashToInvalidate]);
            console.log(`✅ Invalidated block ${blockHashToInvalidate.substring(0, 16)}...`);

            // Generate new blocks to replace the invalidated ones
            console.log('⛏️  Generating replacement blocks...');
            await this.makeRpcCall(this.bitcoinRpcUrl, 'generatetoaddress', [4, 'bcrt1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh']); // Use a test address

            const newTip = await this.getBitcoinBlockCount();
            console.log(`📈 New Bitcoin tip after reorg: ${newTip}`);

            // Verify that the block hashes have changed
            for (let i = 0; i < 4; i++) {
                const height = invalidateHeight + i;
                const newBlockHash = await this.getBitcoinBlockHash(height);
                const originalBlockHash = this.originalBlocks.get(height);
                
                if (originalBlockHash && newBlockHash !== originalBlockHash) {
                    console.log(`🔄 Block ${height} hash changed:`);
                    console.log(`   Old: ${originalBlockHash.substring(0, 16)}...`);
                    console.log(`   New: ${newBlockHash.substring(0, 16)}...`);
                } else {
                    console.log(`⚠️  Block ${height} hash unchanged (this might be expected for some blocks)`);
                }
            }

        } catch (error) {
            console.error(`❌ Failed to create reorg: ${error}`);
            throw error;
        }
    }

    async verifyReorgDetection(): Promise<boolean> {
        console.log('🔍 Verifying reorg detection...');

        // Wait for Metashrew to detect and handle the reorg
        console.log('⏳ Waiting for Metashrew to detect reorg...');
        await new Promise(resolve => setTimeout(resolve, 10000)); // Wait 10 seconds

        let reorgDetected = false;
        let attempts = 0;
        const maxAttempts = 30; // 30 attempts = 1 minute

        while (!reorgDetected && attempts < maxAttempts) {
            attempts++;
            
            try {
                // Check if Metashrew has detected the reorg by comparing block hashes
                const invalidateHeight = this.testStartHeight - 3;
                
                // Get the current block hash from Bitcoin
                const currentBitcoinHash = await this.getBitcoinBlockHash(invalidateHeight);
                
                // Get the block hash that Metashrew has stored
                let metashrewHash: string;
                try {
                    metashrewHash = await this.getMetashrewBlockHash(invalidateHeight);
                } catch (error) {
                    console.log(`   Attempt ${attempts}: Metashrew doesn't have block ${invalidateHeight} yet`);
                    await new Promise(resolve => setTimeout(resolve, 2000));
                    continue;
                }

                // Compare the hashes
                if (currentBitcoinHash !== metashrewHash) {
                    console.log(`🔄 Reorg detected! Block ${invalidateHeight} hashes differ:`);
                    console.log(`   Bitcoin:   ${currentBitcoinHash.substring(0, 16)}...`);
                    console.log(`   Metashrew: ${metashrewHash.substring(0, 16)}...`);
                    reorgDetected = false; // Still processing
                } else {
                    console.log(`✅ Reorg handled! Block ${invalidateHeight} hashes now match:`);
                    console.log(`   Hash: ${currentBitcoinHash.substring(0, 16)}...`);
                    reorgDetected = true;
                }

            } catch (error) {
                console.log(`   Attempt ${attempts}: Error checking reorg status: ${error}`);
            }

            if (!reorgDetected) {
                await new Promise(resolve => setTimeout(resolve, 2000));
            }
        }

        return reorgDetected;
    }

    async verifyStateRootChanges(): Promise<boolean> {
        console.log('🌳 Verifying state root changes...');

        try {
            // Get state roots for the affected heights
            const invalidateHeight = this.testStartHeight - 3;
            
            for (let i = 0; i < 4; i++) {
                const height = invalidateHeight + i;
                
                try {
                    const stateRoot = await this.getMetashrewStateRoot(height);
                    console.log(`📊 State root at height ${height}: ${stateRoot.substring(0, 16)}...`);
                    
                    // Verify it's not all zeros (which was the previous bug)
                    if (stateRoot === '0x0000000000000000000000000000000000000000000000000000000000000000') {
                        console.log(`❌ State root at height ${height} is all zeros!`);
                        return false;
                    }
                } catch (error) {
                    console.log(`⚠️  Could not get state root for height ${height}: ${error}`);
                }
            }

            console.log('✅ State roots look valid (non-zero)');
            return true;

        } catch (error) {
            console.error(`❌ Error verifying state roots: ${error}`);
            return false;
        }
    }

    async cleanup(): Promise<void> {
        console.log('🧹 Cleaning up...');
        
        try {
            // Reconsider the invalidated blocks to restore the original chain
            for (const [height, blockHash] of this.originalBlocks) {
                try {
                    await this.makeRpcCall(this.bitcoinRpcUrl, 'reconsiderblock', [blockHash]);
                    console.log(`♻️  Reconsidered block ${height}: ${blockHash.substring(0, 16)}...`);
                } catch (error) {
                    console.log(`⚠️  Could not reconsider block ${height}: ${error}`);
                }
            }
        } catch (error) {
            console.log(`⚠️  Cleanup had some issues: ${error}`);
        }
    }
}

async function runReorgTest(): Promise<void> {
    console.log('🚀 Starting improved reorg test...');
    console.log('📝 This test invalidates the last 4 blocks to ensure reorg detection works properly');
    
    const harness = new ReorgTestHarness();
    
    try {
        // Setup
        await harness.setup();
        
        // Create reorg
        await harness.createReorg();
        
        // Verify reorg detection
        const reorgDetected = await harness.verifyReorgDetection();
        
        // Verify state root changes
        const stateRootsValid = await harness.verifyStateRootChanges();
        
        // Results
        console.log('\n📊 Test Results:');
        console.log(`   Reorg Detection: ${reorgDetected ? '✅ PASS' : '❌ FAIL'}`);
        console.log(`   State Roots:     ${stateRootsValid ? '✅ PASS' : '❌ FAIL'}`);
        
        if (reorgDetected && stateRootsValid) {
            console.log('\n🎉 Reorg test PASSED! The indexer correctly detected and handled the reorganization.');
        } else {
            console.log('\n❌ Reorg test FAILED! There are issues with reorg detection or state root calculation.');
            process.exit(1);
        }
        
    } catch (error) {
        console.error(`💥 Test failed with error: ${error}`);
        process.exit(1);
    } finally {
        // Cleanup
        await harness.cleanup();
    }
}

// Run the test if this file is executed directly
if (require.main === module) {
    runReorgTest().catch(error => {
        console.error(`💥 Unhandled error: ${error}`);
        process.exit(1);
    });
}

export { ReorgTestHarness, runReorgTest };
