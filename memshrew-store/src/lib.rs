use anyhow::Result;
use metashrew_runtime::{BatchLike, KeyValueStoreLike};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct MemStore {
    db: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Default)]
pub struct MemStoreBatch {
    operations: Vec<(Vec<u8>, Vec<u8>)>,
}

impl BatchLike for MemStoreBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations
            .push((key.as_ref().to_vec(), value.as_ref().to_vec()));
    }
    fn default() -> Self {
        Default::default()
    }
}

impl KeyValueStoreLike for MemStore {
    type Batch = MemStoreBatch;
    type Error = anyhow::Error;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        let mut db = self.db.lock().unwrap();
        for (key, value) in batch.operations {
            db.insert(key, value);
        }
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let db = self.db.lock().unwrap();
        Ok(db.get(key.as_ref()).cloned())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut db = self.db.lock().unwrap();
        db.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        let mut db = self.db.lock().unwrap();
        db.remove(key.as_ref());
        Ok(())
    }
}
