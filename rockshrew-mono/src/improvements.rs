use anyhow::{anyhow, Result};
use log::{debug, info, warn, error};
use crate::MetashrewRocksDBSync;

impl MetashrewRocksDBSync {
    /// Improved reorg detection that properly handles chain reorganizations
    /// Returns the height from which we should start/resume indexing
    pub async fn detect_and_handle_reorg(&self) -> Result<u32> {
        // Get our current indexed height (the last block we successfully processed)
        let current_indexed_height = self.query_height().await?;
        
        // If we haven't indexed any blocks yet, start from the beginning
        if current_indexed_height == 0 {
            info!("No blocks indexed yet, starting from genesis");
            return Ok(0);
        }
        
        // Get the current blockchain tip from the remote node
        let remote_tip = self.fetch_blockcount().await?;
        
        // Start checking from our current indexed height and work backwards
        let mut check_height = current_indexed_height;
        let mut divergence_point: Option<u32> = None;
        
        info!("Checking for reorg: local tip={}, remote tip={}", current_indexed_height, remote_tip);
        
        // Check up to 100 blocks back or until we reach genesis
        let max_reorg_depth = 100;
        let start_check = if current_indexed_height > max_reorg_depth { 
            current_indexed_height - max_reorg_depth 
        } else { 
            0 
        };
        
        while check_height > start_check {
            // Get our local blockhash for this height
            let local_blockhash = self.get_blockhash(check_height).await;
            
            // If we don't have a local blockhash for a height we think we indexed,
            // this indicates a serious database inconsistency
            if local_blockhash.is_none() {
                error!("Missing local blockhash for height {} that should be indexed", check_height);
                // Treat this as a divergence point to be safe
                divergence_point = Some(check_height);
                break;
            }
            
            // Fetch the remote blockhash for this height
            let remote_blockhash = match self.fetch_blockhash(check_height).await {
                Ok(hash) => hash,
                Err(e) => {
                    // If we can't fetch the remote blockhash, the remote chain might not
                    // have this block yet (remote tip < our tip), which is fine
                    if check_height > remote_tip {
                        debug!("Remote chain doesn't have block {} yet (remote tip: {})", check_height, remote_tip);
                        check_height -= 1;
                        continue;
                    } else {
                        return Err(anyhow!("Failed to fetch remote blockhash for height {}: {}", check_height, e));
                    }
                }
            };
            
            // Compare the blockhashes
            if let Some(local_hash) = local_blockhash {
                if local_hash == remote_blockhash {
                    debug!("Blockhash match at height {}", check_height);
                    // Found a matching block, this is our common ancestor
                    break;
                } else {
                    warn!("Blockhash mismatch at height {}: local={}, remote={}", 
                          check_height, hex::encode(&local_hash), hex::encode(&remote_blockhash));
                    divergence_point = Some(check_height);
                }
            }
            
            check_height -= 1;
        }
        
        // Handle the results
        match divergence_point {
            Some(diverge_height) => {
                warn!("Chain reorganization detected! Divergence at height {}", diverge_height);
                warn!("Need to rollback from height {} to height {}", current_indexed_height, diverge_height);
                
                // TODO: Implement actual rollback logic here
                // This would involve:
                // 1. Rolling back all SMT/BST changes after diverge_height
                // 2. Removing state roots after diverge_height  
                // 3. Updating the current height marker
                
                // For now, we'll return the divergence point as the height to resume from
                // The caller should implement the actual rollback
                Ok(diverge_height)
            },
            None => {
                // No divergence found, we can continue from where we left off
                info!("No chain reorganization detected, continuing from height {}", current_indexed_height + 1);
                Ok(current_indexed_height + 1)
            }
        }
    }
    
    /// Simplified best_height that just returns the next block to process
    /// after handling any potential reorgs
    pub async fn get_next_block_height(&self) -> Result<u32> {
        // First, detect and handle any reorgs
        let resume_height = self.detect_and_handle_reorg().await?;
        
        // Get the current remote tip
        let remote_tip = self.fetch_blockcount().await?;
        
        // If we're caught up or ahead, wait for new blocks
        if resume_height > remote_tip {
            info!("Caught up to remote tip at height {}, waiting for new blocks", remote_tip);
            return Ok(remote_tip + 1);
        }
        
        // Return the next block to process
        Ok(resume_height)
    }
    
    /// Rollback the database state to a specific height
    /// This should remove all state changes after the specified height
    pub async fn rollback_to_height(&self, target_height: u32) -> Result<()> {
        info!("Rolling back database state to height {}", target_height);
        
        // Get database handle
        let db = self.poll_connection().await?;
        let smt_helper = crate::smt_helper::SMTHelper::new(db.clone());
        
        // Get current height
        let current_height = self.query_height().await?;
        
        if target_height >= current_height {
            info!("Target height {} >= current height {}, no rollback needed", target_height, current_height);
            return Ok(());
        }
        
        // TODO: Implement the actual rollback logic
        // This is complex and would involve:
        // 1. Identifying all keys that were modified after target_height
        // 2. Removing or reverting those changes
        // 3. Removing state roots after target_height
        // 4. Updating the height marker
        
        warn!("Rollback logic not yet implemented - this is a placeholder");
        
        // For now, just log what we would do
        for height in (target_height + 1)..=current_height {
            let keys_at_height = smt_helper.list_keys_at_height(height)?;
            info!("Would rollback {} keys modified at height {}", keys_at_height.len(), height);
        }
        
        Ok(())
    }
}
