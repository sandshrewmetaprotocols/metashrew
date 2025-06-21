use anyhow::Result;
use metashrew_runtime::{MetashrewRuntime, KeyValueStoreLike};
use sha2::{Digest, Sha256};
pub mod smt;
use smt::SMTHelper;

pub struct MetashrewTestHarness<T: KeyValueStoreLike>
where
    T: KeyValueStoreLike + Clone + Send + Sync + 'static,
    T::Error: std::error::Error + Send + Sync,
{
    pub runtime: MetashrewRuntime<T>,
    pub chain: Vec<Vec<u8>>,
    pub smt: SMTHelper<T>,
    pub remote_chain: Vec<Vec<u8>>,
}

impl<T> MetashrewTestHarness<T>
where
    T: KeyValueStoreLike + Clone + Send + Sync + 'static,
    T::Error: std::error::Error + Send + Sync,
{
    pub fn new(wasm_path: &str, store: T) -> Result<Self> {
        let runtime = MetashrewRuntime::load(wasm_path.into(), store.clone())?;
        let smt = SMTHelper::new(store);
        Ok(Self {
            runtime,
            chain: vec![],
            smt,
            remote_chain: vec![],
        })
    }

    fn hash_block(block_data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(block_data);
        hasher.finalize().into()
    }

    pub fn process_block(&mut self, block_data: &[u8]) -> Result<()> {
        let height = self.chain.len() as u32;
        self.runtime.context.lock().unwrap().height = height;
        self.runtime.context.lock().unwrap().block = block_data.to_vec();
        self.runtime.run()?;
        self.chain.push(block_data.to_vec());
        let block_hash = Self::hash_block(block_data);
        let block_hash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        self.smt.bst_put(&block_hash_key, &block_hash, height)?;
        self.smt.calculate_and_store_state_root(height)?;
        Ok(())
    }

    pub fn get_block_by_height(&self, height: u32) -> Option<&Vec<u8>> {
        self.chain.get(height as usize)
    }

    pub fn get_local_block_hash(&mut self, height: u32) -> Result<Option<Vec<u8>>> {
        let block_hash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        self.smt.bst_get_at_height(&block_hash_key, height)
    }

    pub fn get_remote_block_hash(&self, height: u32) -> Option<[u8; 32]> {
        self.remote_chain.get(height as usize).map(|b| Self::hash_block(b))
    }

    pub fn find_common_ancestor(&mut self) -> Result<u32> {
        let local_height = self.chain.len() as u32;
        let remote_height = self.remote_chain.len() as u32;
        let mut height = std::cmp::min(local_height, remote_height);

        loop {
            if height == 0 {
                return Ok(0);
            }
            let local_hash = self.get_local_block_hash(height - 1)?;
            let remote_hash = self.get_remote_block_hash(height - 1).map(|h| h.to_vec());

            if local_hash.is_some() && remote_hash.is_some() && local_hash == remote_hash {
                return Ok(height);
            }
            height -= 1;
        }
    }

    pub fn reorg(&mut self) -> Result<()> {
        let common_ancestor_height = self.find_common_ancestor()?;
        self.chain.truncate(common_ancestor_height as usize);
        for i in (common_ancestor_height as usize)..self.remote_chain.len() {
            let block_data = self.remote_chain[i].clone();
            self.process_block(&block_data)?;
        }
        Ok(())
    }
}