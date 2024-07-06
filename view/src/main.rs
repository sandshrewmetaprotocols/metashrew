use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result};
//use itertools::Itertools;
use metashrew_runtime::{BatchLike, KeyValueStoreLike};
//use rlp::Rlp;
use rocksdb::{ColumnFamily, Options, WriteBatch, DB};
use serde::{Deserialize, Serialize};
//use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::ffi::OsString;
use std::io::{prelude::*, BufReader};
use std::path::PathBuf;
use std::process::id;
use std::time::{SystemTime, UNIX_EPOCH};
use substring::Substring;
use tiny_keccak::{Hasher, Sha3};
use anyhow;
use log::{info};
/*
use wasmtime::{
    Caller, Config, Engine, Extern, Global, GlobalType, Instance, Linker, Memory, MemoryType,
    Module, Mutability, SharedMemory, Store, Val, ValType,
};
*/

use env_logger;

static mut INIT_DB: Option<&'static DB> = None;
pub struct RocksDBRuntimeAdapter(&'static DB);
pub struct RocksDBBatch(pub WriteBatch);

pub fn index_cf(db: &DB) -> &ColumnFamily {
    db.cf_handle(INDEX_CF).expect("missing INDEX_CF")
}

impl Clone for RocksDBRuntimeAdapter {
    fn clone(&self) -> Self {
        return Self(self.0);
    }
}

impl BatchLike for RocksDBBatch {
    fn default() -> RocksDBBatch {
        RocksDBBatch(rocksdb::WriteBatch::default())
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, _k: K, _v: V) {}
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    hash: [u8; 32],
    #[allow(dead_code)]
    program: Vec<u8>,
    path: PathBuf,
}

/*
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
*/
const CONFIG_CF: &str = "config";
const HEADERS_CF: &str = "headers";
const TXID_CF: &str = "txid";
const FUNDING_CF: &str = "funding";
const SPENDING_CF: &str = "spending";
const INDEX_CF: &str = "index";
const HEIGHT_CF: &str = "height";
const HEIGHT_KEY: &[u8] = b"H";

const COLUMN_FAMILIES: &[&str] = &[
    CONFIG_CF,
    HEADERS_CF,
    TXID_CF,
    FUNDING_CF,
    SPENDING_CF,
    INDEX_CF,
    HEIGHT_CF,
];

fn create_cf_descriptors() -> Vec<&'static str> {
    COLUMN_FAMILIES.into()
}

pub fn headers_cf(db: &DB) -> &rocksdb::ColumnFamily {
    db.cf_handle(HEADERS_CF).expect("missing HEADERS_CF")
}

pub fn height_cf(db: &DB) -> &rocksdb::ColumnFamily {
    db.cf_handle(HEIGHT_CF).expect("missing HEIGHT_CF")
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
        let internal_db = unsafe { RocksDBRuntimeAdapter(INIT_DB.unwrap()) };
        let runtime =
            metashrew_runtime::MetashrewRuntime::load(context.path.clone(), internal_db).unwrap();
        let height: u32 = if body.params[2] == "latest" {
            unsafe {
                let height_bytes: Vec<u8> = INIT_DB
                    .unwrap()
                    .get_cf(height_cf(INIT_DB.expect("db isn't there")), HEIGHT_KEY)
                    .expect("get tip failed")
                    .unwrap();
                u32::from_le_bytes(height_bytes.into_boxed_slice()[0..4].try_into().unwrap())
            }
        } else {
            body.params[2].parse::<u32>().unwrap()
        };
        return Ok(HttpResponse::Ok().json(JsonRpcResult {
            id: body.id,
            result: hex::encode(
                runtime
                    .view(
                        body.params[0].clone(),
                        &hex::decode(
                            body.params[1]
                                .to_string()
                                .substring(2, body.params[1].len()),
                        )
                        .unwrap(),
                        height,
                    )
                    .unwrap(),
            ),
            jsonrpc: "2.0".to_string(),
        }));
    }
}

fn get_secondary_directory() -> Result<OsString, anyhow::Error> {
  let db_path = env::var("DB_LOCATION")?;
  let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?;
  let mut path = PathBuf::from(db_path.as_str());
  let dir = String::from(path.file_name().ok_or(anyhow::anyhow!("filename couldn't be retrieved from path"))?.to_str().ok_or(anyhow::anyhow!("couldn't convert path to string"))?);
  path.pop();
  path.push(String::from("view-") + &dir + &String::from("-") + &id().to_string() + &since_epoch.as_millis().to_string());
  Ok(path.into_os_string())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    // get the
    let path = match env::var("PROGRAM_PATH") {
        Ok(val) => val,
        Err(_e) => PathBuf::from("/mnt/volume/indexer.wasm")
            .to_str()
            .unwrap()
            .try_into()
            .unwrap(),
    };
    let program = File::open(path.clone()).expect("msg");
    let mut buf = BufReader::new(program);
    let mut bytes: Vec<u8> = vec![];
    let _ = buf.read_to_end(&mut bytes);
    let mut hasher = Sha3::v256();
    let mut output = [0; 32];
    hasher.update(bytes.as_slice());
    hasher.finalize(&mut output);
    info!("program hash: 0x{}", hex::encode(output));
    let secondary = get_secondary_directory().unwrap().into_string().unwrap();
    info!("acquiring database handle -- this takes a while ...");
    unsafe {
        INIT_DB = Some(Box::leak(Box::new(
            DB::open_cf_as_secondary(
                &Options::default(),
                env::var("DB_LOCATION").unwrap(),
                secondary,
                create_cf_descriptors(),
            )
            .unwrap(),
        )));
    }
    info!("rocksdb opened in secondary");
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
            Err(_e) => String::from("127.0.0.1"),
        },
        match env::var("PORT") {
            Ok(val) => val.parse::<u16>().unwrap(),
            Err(_e) => 8080,
        },
    ))?
    .run()
    .await
}
