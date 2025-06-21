//! Generic runtime context for MetashrewRuntime

use crate::traits::KeyValueStoreLike;

/// Generic runtime context that works with any storage backend
pub struct MetashrewRuntimeContext<T: KeyValueStoreLike> {
    pub db: T,
    pub height: u32,
    pub block: Vec<u8>,
    pub state: u32,
}

impl<T: KeyValueStoreLike> Clone for MetashrewRuntimeContext<T> 
where 
    T: Clone 
{
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            height: self.height,
            block: self.block.clone(),
            state: self.state,
        }
    }
}

impl<T: KeyValueStoreLike> MetashrewRuntimeContext<T> {
    pub fn new(db: T, height: u32, block: Vec<u8>) -> Self {
        Self {
            db,
            height,
            block,
            state: 0,
        }
    }
}