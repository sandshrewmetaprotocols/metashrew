use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result};
//use itertools::Itertools;
use metashrew_keydb::{RedisRuntimeAdapter, query_height};
use metashrew_runtime::{MetashrewRuntime};
//use rlp::Rlp;
use serde::{Deserialize, Serialize};
use serde_json;
//use std::collections::HashSet;
use anyhow;
use log::{debug, info};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::PathBuf;
use std::process::id;
use std::time::{SystemTime, UNIX_EPOCH};
use substring::Substring;
use tiny_keccak::{Hasher, Sha3};
/*
use wasmtime::{
    Caller, Config, Engine, Extern, Global, GlobalType, Instance, Linker, Memory, MemoryType,
    Module, Mutability, SharedMemory, Store, Val, ValType,
};
*/

use env_logger;

#[derive(Deserialize, Serialize)]
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
    runtime: MetashrewRuntime<RedisRuntimeAdapter>
}

static mut _HEIGHT: u32 = 0;

pub fn height() -> u32 {
    unsafe { _HEIGHT }
}

pub fn set_height(h: u32) -> u32 {
    unsafe {
        _HEIGHT = h;
        _HEIGHT
    }
}

pub fn fetch_and_set_height(internal_db: &RedisRuntimeAdapter) -> u32 {
    unsafe {
        let height = query_height(internal_db, 0);
        set_height(height)
    }
}

#[post("/")]
async fn view(
    body: web::Json<JsonRpcRequest>,
    context: web::Data<Context>,
) -> Result<impl Responder> {
    {
        debug!("{}", serde_json::to_string(&body).unwrap());
    }
    if body.method != "metashrew_view" {
        let resp = JsonRpcError {
            id: body.id,
            error: "Unsupported method".to_string(),
            jsonrpc: "2.0".to_string(),
        };
        return Ok(HttpResponse::Ok().json(resp));
    } else {
        let height: u32 = if body.params[2] == "latest" {
            fetch_and_set_height(&context.runtime.context.lock().unwrap().db)
        } else {
            let h = body.params[2].parse::<u32>().unwrap();
            if h > height() {
                fetch_and_set_height(&context.runtime.context.lock().unwrap().db);
            }
            h
        };
        let result = JsonRpcResult {
            id: body.id,
            result: String::from("0x")
                + hex::encode(
                    context
                        .runtime
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
                )
                .as_str(),
            jsonrpc: "2.0".to_string(),
        };
        return Ok(HttpResponse::Ok().json(result));
    }
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
    let path_clone: PathBuf = path.into();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Context {
                hash: output,
                program: bytes.clone(),
                runtime: MetashrewRuntime::load(path_clone.clone(), unsafe {
                    RedisRuntimeAdapter(Arc::new(Mutex::new(redis::Client::open(match env::var("REDIS_URI") { Some(v) => v, None => "redis://127.0.0.1:6379" }).unwrap().get_connection().unwrap())))
                })
                .unwrap(),
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
