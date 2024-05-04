use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result};
use itertools::Itertools;
use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};
use rlp::Rlp;
use rocksdb::{Options, DB};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::PathBuf;
use substring::Substring;
use tiny_keccak::{Hasher, Sha3};
use wasmtime::{
    Caller, Config, Engine, Extern, Global, GlobalType, Instance, Linker, Memory, MemoryType,
    Module, Mutability, SharedMemory, Store, Val, ValType,
};
pub struct RocksDBRuntimeAdapter(&'static DB);
pub struct RocksDBBatch(pub rocksdb::WriteBatch);

impl BatchLike for RocksDBBatch {
    fn default() -> RocksDBBatch {
        RocksDBBatch(rocksdb::WriteBatch::default())
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        return self.0.put(k, v);
    }
}

impl Clone for RocksDBRuntimeAdapter {
  fn clone(&self) -> Self {
    return Self(self.0);
  }
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;
    fn write(&self, batch: RocksDBBatch) -> Result<(), Self::Error> {
        let _ = self.0.write(batch.0);
        Ok(())
    }
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.0.get(key)
    }
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        let _ = self.0.delete(key);
        Ok(())
    }
    fn put<K, V>(&self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.0.put(key, value)
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
                .substring(2, (body.params[0].len() - 2)),
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
        let db_path = match env::var("DB_LOCATION") {
            Ok(val) => val,
            Err(e) => "/mnt/volume/rocksdb".to_string(),
        };
        let db: &'static DB = Box::leak(Box::new(
            DB::open_for_read_only(&Options::default(), db_path, false).unwrap(),
        ));
        let internal_db = RocksDBRuntimeAdapter(db);
        let mut runtime =
            metashrew_runtime::MetashrewRuntime::load(context.path.clone(), internal_db).unwrap();
        return Ok(HttpResponse::Ok().json(JsonRpcResult {
            id: body.id,
            result: hex::encode(
                runtime
                    .view(
                        body.params[1].clone(),
                        &hex::decode(
                            body.params[2]
                                .to_string()
                                .substring(2, body.params[2].len() - 2),
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
