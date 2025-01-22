use criterion::{black_box, criterion_group, criterion_main, Criterion};
use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct MockDB {
    data: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MockDB {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Default, Clone)]
struct MockBatch {
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

impl BatchLike for MockBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations
            .push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
    }

    fn default() -> Self {
        Self::default()
    }
}

impl KeyValueStoreLike for MockDB {
    type Error = std::io::Error;
    type Batch = MockBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        let mut data = self.data.lock().unwrap();
        for (key, value_opt) in batch.operations {
            match value_opt {
                Some(value) => data.insert(key, value),
                None => data.remove(&key),
            };
        }
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let data = self.data.lock().unwrap();
        Ok(data.get(key.as_ref()).cloned())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        let mut data = self.data.lock().unwrap();
        data.remove(key.as_ref());
        Ok(())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut data = self.data.lock().unwrap();
        data.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }
}

fn bench_db_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("db_operations");

    group.bench_function("batch_insert_1000", |b| {
        let db = MockDB::new();
        let mut batch = MockBatch::default();

        b.iter(|| {
            for i in 0..1000 {
                let key = format!("key_{}", i).into_bytes();
                let value = format!("value_{}", i).into_bytes();
                batch.put(key, value);
            }
            black_box(db.write(batch.clone())).unwrap();
        });
    });

    group.bench_function("get_1000_sequential", |b| {
        let mut db = MockDB::new();
        // Setup data
        let mut batch = MockBatch::default();
        for i in 0..1000 {
            let key = format!("key_{}", i).into_bytes();
            let value = format!("value_{}", i).into_bytes();
            batch.put(key, value);
        }
        db.write(batch).unwrap();

        b.iter(|| {
            for i in 0..1000 {
                let key = format!("key_{}", i).into_bytes();
                black_box(db.get(&key)).unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_db_operations);
criterion_main!(benches);
