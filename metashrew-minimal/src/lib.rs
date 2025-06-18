use metashrew_core::{
  context::Context,
  error::Error,
  indexer::{Indexer, IndexerResult},
  kv_store::KVStore,
};

pub struct MinimalIndexer;

impl Indexer for MinimalIndexer {
  type Key = Vec<u8>;
  type Value = Vec<u8>;

  fn index(context: &mut Context<Self>) -> IndexerResult<()> {
    let block_bytes = context.load_input();
    let height = context.height;
    let key = format!("/blocks/{}", height).into_bytes();
    context.kv_store.insert(key, block_bytes)?;
    Ok(())
  }
}

#[no_mangle]
pub extern "C" fn _start() {
  let result = MinimalIndexer::execute();
  if let Err(e) = result {
    panic!("Indexer failed: {:?}", e);
  }
}

#[no_mangle]
pub extern "C" fn view_block_by_height() {
  let mut context = Context::load();
  let height_bytes = context.load_input();
  let height = u32::from_be_bytes(height_bytes.try_into().unwrap());
  let key = format!("/blocks/{}", height).into_bytes();
  let block_bytes = context.kv_store.get(&key).unwrap().unwrap();
  context.return_output(&block_bytes);
}
