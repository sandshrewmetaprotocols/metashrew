use actix_cors::Cors;
use actix_web::error;
use actix_web::http::{header::ContentType, StatusCode};
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result};
use metashrew_rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use metashrew_runtime::MetashrewRuntime;
use std::fmt;
use anyhow;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use serde_json;
use std::env;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::PathBuf;
use std::sync::Arc;
use substring::Substring;
use tiny_keccak::{Hasher, Sha3};
use rocksdb::Options;

struct MetashrewViewError(pub anyhow::Error);

impl From<anyhow::Error> for MetashrewViewError {
    fn from(v: anyhow::Error) -> Self {
        Self(v)
    }
}

impl fmt::Display for MetashrewViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        <anyhow::Error as fmt::Display>::fmt(&self.0, f)
    }
}

impl std::fmt::Debug for MetashrewViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        <anyhow::Error as std::fmt::Debug>::fmt(&self.0, f)
    }
}

impl error::ResponseError for MetashrewViewError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(String::from("Internal server error"))
    }
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

fn from_anyhow(v: anyhow::Error) -> MetashrewViewError {
    v.into()
}

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
    error: String,
}

struct Context {
    #[allow(dead_code)]
    hash: [u8; 32],
    #[allow(dead_code)]
    program: Vec<u8>,
    runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
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

pub async fn fetch_and_set_height(internal_db: &RocksDBRuntimeAdapter) -> Result<u32> {
    let height = query_height(&internal_db.db, 0)
        .await
        .map_err(|e| from_anyhow(e))?;
    Ok(set_height(height))
}

#[post("/")]
async fn jsonrpc_call(
    body: web::Json<JsonRpcRequest>,
    context: web::Data<Context>,
) -> Result<impl Responder> {
    {
        debug!("{}", serde_json::to_string(&body).unwrap());
    }
    if body.method == "metashrew_view" {
        let height: u32 = if body.params[2] == "latest" {
            fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
        } else {
            let h = body.params[2].parse::<u32>().unwrap();
            if h > height() {
                fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?;
            }
            h
        };
        let (res_string, err) = match context.runtime.view(
            body.params[0].clone(),
            &hex::decode(
                body.params[1]
                    .to_string()
                    .substring(2, body.params[1].len()),
            )
            .unwrap(),
            height,
        ) {
            Ok(str) => (str, "".to_string()),
            Err(err) => {
                println!("{:#?}", err);
                (vec![], err.to_string())
            }
        };
        let result = JsonRpcResult {
            id: body.id,
            result: String::from("0x") + hex::encode(res_string).as_str(),
            error: err,
            jsonrpc: "2.0".to_string(),
        };
        return Ok(HttpResponse::Ok().json(result));
    } else if body.method == "metashrew_height" {
        let height = fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?;
        let result = JsonRpcResult {
            id: body.id,
            result: height.to_string(),
            error: "".to_string(),
            jsonrpc: "2.0".to_string(),
        };
        return Ok(HttpResponse::Ok().json(result));
    } else if body.method == "metashrew_preview" {
        let height: u32 = if body.params[3] == "latest" {
            fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
        } else {
            let h = body.params[3].parse::<u32>().unwrap();
            if h > height() {
                fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?;
            }
            h
        };

        let block_hex = body.params[0].clone();
        // Remove 0x prefix if present and decode hex
        let block_data =
            hex::decode(block_hex.trim_start_matches("0x")).map_err(|e| from_anyhow(e.into()))?;

        // Use preview to execute block and view
        let (res_string, err) = match context.runtime.preview(
            &block_data,
            body.params[1].clone(),
            &hex::decode(
                body.params[2]
                    .to_string()
                    .substring(2, body.params[2].len()),
            )
            .unwrap(),
            height,
        ) {
            Ok(str) => (str, "".to_string()),
            Err(err) => {
                println!("{:#?}", err);
                (vec![], err.to_string())
            }
        };

        let result = JsonRpcResult {
            id: body.id,
            result: String::from("0x") + hex::encode(res_string).as_str(),
            error: err,
            jsonrpc: "2.0".to_string(),
        };

        return Ok(HttpResponse::Ok().json(result));
    } else {
        let result = JsonRpcResult {
            id: body.id,
            result: "".to_owned(),
            error: format!("RPC method {} not found", body.method),
            jsonrpc: "2.0".to_string(),
        };
        return Ok(HttpResponse::Ok().json(result));
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    
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
    
    if let Ok(label) = env::var("ROCKS_LABEL") {
        set_label(label.clone());
    }
    
    let db_path: String = match env::var("ROCKS_DB_PATH") {
        Ok(v) => v,
        Err(_) => "rocksdb_data".into(),
    };

    // Configure RocksDB options for optimal performance
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_max_open_files(10000);
    opts.set_use_fsync(false);
    opts.set_bytes_per_sync(8388608); // 8MB
    opts.optimize_for_point_lookup(1024);
    opts.set_table_cache_num_shard_bits(6);
    opts.set_max_write_buffer_number(6);
    opts.set_write_buffer_size(256 * 1024 * 1024);
    opts.set_target_file_size_base(256 * 1024 * 1024);
    opts.set_min_write_buffer_number_to_merge(2);
    opts.set_level_zero_file_num_compaction_trigger(4);
    opts.set_level_zero_slowdown_writes_trigger(20);
    opts.set_level_zero_stop_writes_trigger(30);
    opts.set_max_background_jobs(4);
    opts.set_max_background_compactions(4);
    opts.set_disable_auto_compactions(false);

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::default().allowed_origin_fn(|origin, _| {
                if let Ok(origin_str) = origin.to_str() {
                    origin_str.starts_with("http://localhost:")
                } else {
                    false
                }
            }))
            .app_data(web::Data::new(Context {
                hash: output,
                program: bytes.clone(),
                runtime: MetashrewRuntime::load(
                    path_clone.clone(),
                    RocksDBRuntimeAdapter::open(db_path.clone(), opts.clone()).unwrap(),
                )
                .unwrap(),
            }))
            .service(jsonrpc_call)
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