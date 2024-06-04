use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result};
use itertools::Itertools;
use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};
use rlp::Rlp;
use rocksdb::{ColumnFamily, Options, WriteBatch, DB};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::PathBuf;
use std::process::id;
use std::time::{SystemTime, UNIX_EPOCH};
use substring::Substring;
use bitcoin::consensus::{deserialize};
use tiny_keccak::{Hasher, Sha3};
use wasmtime::{
    Caller, Config, Engine, Extern, Global, GlobalType, Instance, Linker, Memory, MemoryType,
    Module, Mutability, SharedMemory, Store, Val, ValType,
};

static mut init_db: Option<&'static DB> = None;
pub struct RocksDBRuntimeAdapter(&'static DB);
pub struct RocksDBBatch(pub WriteBatch);

pub const TIP_KEY: &[u8] = b"T";

pub fn index_cf(db: &DB) -> &ColumnFamily {
    db.cf_handle(INDEX_CF).expect("missing INDEX_CF")
}

pub fn headers_cf(db: &DB) -> &rocksdb::ColumnFamily {
    db.cf_handle(HEADERS_CF).expect("missing HEADERS_CF")
}


impl Clone for RocksDBRuntimeAdapter {
    fn clone(&self) -> Self {
        return Self(self.0);
    }
}

impl BatchLike for RocksDBBatch {
    fn default() -> RocksDBBatch {
        RocksDBBatch(WriteBatch::default())
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {}
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;
    fn write(&self, batch: RocksDBBatch) -> Result<(), Self::Error> {
        let _ = self.0.write(batch.0);
        Ok(())
    }
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.0.get_cf(index_cf(self.0), key)
    }
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        let _ = self.0.delete_cf(index_cf(self.0), key);
        Ok(())
    }
    fn put<K, V>(&self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.0.put_cf(index_cf(self.0), key, value)
    }
}

#[derive(Deserialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
    params: Vec<String>,
    jsonrpc: String,
}
#[derive(Serialize)]
struct JsonRpcError {
    id: u32,
    error: String,
    jsonrpc: String,
}
#[derive(Serialize)]
struct JsonRpcResult {
    id: u32,
    result: String,
    jsonrpc: String,
}

struct Context {
    hash: [u8; 32],
    program: Vec<u8>,
    path: PathBuf,
}

fn default_opts() -> rocksdb::Options {
    let mut block_opts = rocksdb::BlockBasedOptions::default();
    block_opts.set_checksum_type(rocksdb::ChecksumType::CRC32c);

    let mut opts = rocksdb::Options::default();
    //    opts.set_keep_log_file_num(10);
    opts.set_max_open_files(-1);
    opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);
    opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
    //    opts.set_target_file_size_base(256 << 20);
    opts.set_write_buffer_size(256 << 24);
    opts.set_disable_auto_compactions(true); // for initial bulk load
                                             //    opts.set_advise_random_on_open(false); // bulk load uses sequential I/O
    opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(8));
    opts.set_block_based_table_factory(&block_opts);
    opts
}
const CONFIG_CF: &str = "config";
const HEADERS_CF: &str = "headers";
const TXID_CF: &str = "txid";
const FUNDING_CF: &str = "funding";
const SPENDING_CF: &str = "spending";
const INDEX_CF: &str = "index";

const COLUMN_FAMILIES: &[&str] = &[
    CONFIG_CF,
    HEADERS_CF,
    TXID_CF,
    FUNDING_CF,
    SPENDING_CF,
    INDEX_CF,
];

fn create_cf_descriptors() -> Vec<&'static str> {
    COLUMN_FAMILIES.into()
}

#[post("/")]
async fn view(
    body: web::Json<JsonRpcRequest>,
    context: web::Data<Context>,
) -> Result<impl Responder> {
    if body.method != "metashrew_view" {
        let resp = JsonRpcError {
            id: body.id,
            error: "Unsupported method".to_string(),
            jsonrpc: "2.0".to_string(),
        };
        return Ok(HttpResponse::Ok().json(resp));
    } else {
        if hex::decode(
            body.params[0]
                .to_string()
                .substring(2, (body.params[0].len())),
        )
        .unwrap()
            != context.hash
        {
            let resp = JsonRpcError {
                id: body.id,
                error: "Hash doesn't match".to_string(),
                jsonrpc: "2.0".to_string(),
            };
            return Ok(HttpResponse::Ok().json(resp));
        }
        let internal_db = unsafe { RocksDBRuntimeAdapter(init_db.unwrap()) };
        let runtime =
            metashrew_runtime::MetashrewRuntime::load(context.path.clone(), internal_db).unwrap();
        let height: u32;
        if body.params[3] == "latest" {
            unsafe{
                let tip_header = init_db.unwrap().get_cf(headers_cf(init_db.expect("db isn't there")), TIP_KEY).expect("get tip failed");
                // get the height out of the header_row
                let row = init_db.unwrap().get_cf(headers_cf(init_db.expect("db wasn't there")), deserialize::<Vec<u8>>((&(tip_header.unwrap()))).expect("invalid tip")).expect("get header failed");
                height = u32::from_le_bytes(row.unwrap().try_into().unwrap());
            }
        } else {
            height = body.params[3].parse::<u32>().unwrap();
        }
        runtime.context.lock().unwrap().height = height;
        return Ok(HttpResponse::Ok().json(JsonRpcResult {
            id: body.id,
            result: hex::encode(
                runtime
                    .view(
                        body.params[1].clone(),
                        &hex::decode(
                            body.params[2]
                                .to_string()
                                .substring(2, body.params[2].len()),
                        )
                        .unwrap(),
                        body.params[3].parse::<u32>().unwrap(),
                    )
                    .unwrap(),
            ),
            jsonrpc: "2.0".to_string(),
        }));
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // get the
    let path = match env::var("PROGRAM_PATH") {
        Ok(val) => val,
        Err(e) => PathBuf::from("/mnt/volume/indexer.wasm")
            .to_str()
            .unwrap()
            .try_into()
            .unwrap(),
    };
    let program = File::open(path.clone()).expect("msg");
    let mut buf = BufReader::new(program);
    let mut bytes: Vec<u8> = vec![];
    buf.read_to_end(&mut bytes);
    let mut hasher = Sha3::v256();
    let mut output = [0; 32];
    hasher.update(bytes.as_slice());
    hasher.finalize(&mut output);
    println!("program hash: 0x{}", hex::encode(output));
    let db_path = match env::var("DB_LOCATION") {
        Ok(val) => val,
        Err(e) => "/mnt/volume/rocksdb".to_string(),
    };
    let time = SystemTime::now();
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap();
    let secondary = match env::var("DB_LOCATION") {
        Ok(val) => val + &'-'.to_string() + &id().to_string() + &'-'.to_string() + &since_epoch.as_millis().to_string(),
        Err(e) => "/mnt/volume/rocksdb".to_string(),
    };
    unsafe {
        init_db = Some(Box::leak(Box::new(
            DB::open_cf_as_secondary(
                &Options::default(),
                db_path,
                secondary,
                create_cf_descriptors(),
            )
            .unwrap(),
        )));
    }
    println!("initialized rocksdb as secondary");
    let path_clone: PathBuf = path.into();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Context {
                hash: output,
                program: bytes.clone(),
                path: path_clone.clone(),
            }))
            .service(view)
    })
    .bind((
        match env::var("HOST") {
            Ok(val) => val,
            Err(e) => String::from("127.0.0.1"),
        },
        match env::var("PORT") {
            Ok(val) => val.parse::<u16>().unwrap(),
            Err(e) => 8080,
        },
    ))?
    .run()
    .await
}
