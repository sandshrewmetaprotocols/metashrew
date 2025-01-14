use anyhow::Result;
use log::{debug, info};
use metashrew_runtime::{BatchLike, KeyValueStoreLike};
use rocksdb::{DB, Options, WriteBatch, WriteBatchIterator};
use std::sync::{Arc, Mutex};

const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";

#[derive(Clone)]
pub struct RocksDBRuntimeAdapter {
    pub db: Arc<DB>,
    pub height: u32,
}

static mut _LABEL: Option<String> = None;

const TIMEOUT: u64 = 1500;

use std::{thread, time};

pub fn wait_timeout() {
    thread::sleep(time::Duration::from_millis(TIMEOUT));
}

pub fn set_label(s: String) -> () {
    unsafe {
        _LABEL = Some(s + "://");
    }
}

pub fn get_label() -> &'static String {
    unsafe { _LABEL.as_ref().unwrap() }
}

pub fn has_label() -> bool {
    unsafe { _LABEL.is_some() }
}

pub fn to_labeled_key(key: &Vec<u8>) -> Vec<u8> {
    if has_label() {
        let mut result: Vec<u8> = vec![];
        result.extend(get_label().as_str().as_bytes());
        result.extend(key);
        result
    } else {
        key.clone()
    }
}

pub async fn query_height(db: Arc<DB>, start_block: u32) -> Result<u32> {
    let height_key = TIP_HEIGHT_KEY.as_bytes().to_vec();
    let bytes = match db.get(&to_labeled_key(&height_key))? {
        Some(v) => v,
        None => {
            return Ok(start_block);
        }
    };
    if bytes.len() == 0 {
        return Ok(start_block);
    }
    let bytes_ref: &[u8] = &bytes;
    Ok(u32::from_le_bytes(bytes_ref.try_into().unwrap()))
}

impl RocksDBRuntimeAdapter {
    pub fn open(path: String, opts: Options) -> Result<RocksDBRuntimeAdapter> {
        let db = DB::open(&opts, path)?;
        Ok(RocksDBRuntimeAdapter {
            db: Arc::new(db),
            height: 0,
        })
    }

    pub fn is_open(&self) -> bool {
        true // RocksDB doesn't need connection management like Redis
    }

    pub fn set_height(&mut self, height: u32) {
        self.height = height;
    }

    pub fn clone(&self) -> Self {
        RocksDBRuntimeAdapter {
            db: self.db.clone(),
            height: self.height,
        }
    }
}

pub struct RocksDBBatch(pub WriteBatch);

impl BatchLike for RocksDBBatch {
    fn default() -> Self {
        Self(WriteBatch::default())
    }
    
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        self.0.put(to_labeled_key(&k.as_ref().to_vec()), v);
    }
}

pub struct RocksDBBatchCloner<'a>(&'a mut WriteBatch);

impl<'a> WriteBatchIterator for RocksDBBatchCloner<'a> {
  fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
    self.0.put(key.as_ref().clone(), value.as_ref().clone());
  }
  fn delete(&mut self, key: Box<[u8]>) {
    //no-op
  }
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;

    fn write(&mut self, batch: RocksDBBatch) -> Result<(), Self::Error> {
        let key_bytes: Vec<u8> = TIP_HEIGHT_KEY.as_bytes().to_vec();
        let height_bytes: Vec<u8> = (self.height + 1).to_le_bytes().to_vec();
        
        let mut final_batch = WriteBatch::default();
        final_batch.put(&to_labeled_key(&key_bytes), &height_bytes);
        batch.0.iterate(&mut RocksDBBatchCloner(&mut final_batch));
        
        self.db.write(final_batch)
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.db.get(to_labeled_key(&key.as_ref().to_vec())).map(|opt| opt.map(|v| v.to_vec()))
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.db.delete(to_labeled_key(&key.as_ref().to_vec()))
    }

    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) -> Result<(), Self::Error> {
        self.db.put(to_labeled_key(&key.as_ref().to_vec()), value)
    }
}
