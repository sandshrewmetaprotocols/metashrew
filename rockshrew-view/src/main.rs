use actix_cors::Cors;
use actix_web::error;
use actix_web::http::{header::ContentType, StatusCode};
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result};
use metashrew_rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use metashrew_runtime::MetashrewRuntime;
use std::fmt;
use anyhow;
use clap::{Parser, CommandFactory};
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

/// RocksDB-backed view server for metashrew
#[derive(Parser, Debug, Default)]
#[command(author, version, about, long_about = None)]
struct RockshrewViewArgs {
    /// Path to the indexer WASM program
    #[arg(long, env = "PROGRAM_PATH", default_value = "/mnt/volume/indexer.wasm")]
    indexer: PathBuf,
    
    /// Optional RocksDB label for the database
    #[arg(long, env = "ROCKS_LABEL")]
    label: Option<String>,
    
    /// Path to the primary RocksDB database directory
    #[arg(long, env = "ROCKS_DB_PATH", default_value = "rocksdb_data")]
    db_path: String,
    
    /// Path for secondary instance files (required for secondary mode)
    #[arg(long, env = "SECONDARY_PATH", default_value = "rocksdb_secondary")]
    secondary_path: String,
    
    /// Host address to bind the server to
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,
    
    /// Port number to listen on
    #[arg(long, env = "PORT", default_value_t = 8080)]
    port: u16,
}

fn from_anyhow(err: anyhow::Error) -> actix_web::Error {
    error::InternalError::new(
        err.to_string(),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .into()
}

#[derive(Deserialize, Serialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
    params: Vec<serde_json::Value>,
    #[allow(dead_code)]
    jsonrpc: String,
}

#[derive(Serialize)]
struct JsonRpcResult {
    id: u32,
    result: String,
    jsonrpc: String,
}
#[derive(Serialize)]
struct JsonRpcError {
    id: u32,
    error: JsonRpcErrorObject,
    jsonrpc: String,
}

#[derive(Serialize)]
struct JsonRpcErrorObject {
    code: i32,
    message: String,
    data: Option<String>,
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
    let height = query_height(internal_db.db.clone(), 0)
        .await
        .map_err(|e| from_anyhow(e))?;
    Ok(set_height(height))
}

#[post("/")]
async fn jsonrpc_call(
    body: web::Json<JsonRpcRequest>,
    context: web::Data<Context>,
) -> Result<impl Responder> {
    debug!("{}", serde_json::to_string(&body).unwrap());

    if body.method == "metashrew_view" {
        // Ensure we have required params
        if body.params.len() < 3 {
            let error = JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [view_name, input_data, height]".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            };
            return Ok(HttpResponse::Ok().json(error));
        }

        let view_name = match body.params[0].as_str() {
            Some(s) => s.to_string(),
            None => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: view_name must be a string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        let input_hex = match body.params[1].as_str() {
            Some(s) => s.to_string(),
            None => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: input_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        let height: u32 = match &body.params[2] {
            serde_json::Value::String(s) if s == "latest" => {
                fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
            }
            serde_json::Value::Number(n) => {
                let h = n.as_u64().unwrap_or(0) as u32;
                if h > height() {
                    fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
                } else {
                    h
                }
            }
            serde_json::Value::String(s) => {
                match s.parse::<u32>() {
                    Ok(h) => {
                        if h > height() {
                            fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
                        } else {
                            h
                        }
                    }
                    Err(_) => {
                        let error = JsonRpcError {
                            id: body.id,
                            error: JsonRpcErrorObject {
                                code: -32602,
                                message: "Invalid params: height must be a number or 'latest'".to_string(),
                                data: None,
                            },
                            jsonrpc: "2.0".to_string(),
                        };
                        return Ok(HttpResponse::Ok().json(error));
                    }
                }
            }
            _ => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        match context.runtime.view(
            view_name,
            &hex::decode(
                input_hex.trim_start_matches("0x"),
            ).map_err(|e| {
                error::ErrorBadRequest(format!("Invalid hex input: {}", e))
            })?,
            height,
        ) {
            Ok(res_string) => {
                let result = JsonRpcResult {
                    id: body.id,
                    result: String::from("0x") + hex::encode(res_string).as_str(),
                    jsonrpc: "2.0".to_string(),
                };
                Ok(HttpResponse::Ok().json(result))
            }
            Err(err) => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32000,
                        message: err.to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                Ok(HttpResponse::Ok().json(error))
            }
        }
    } else if body.method == "metashrew_height" {
        let height = fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?;
        let result = JsonRpcResult {
            id: body.id,
            result: height.to_string(),
            jsonrpc: "2.0".to_string(),
        };
        Ok(HttpResponse::Ok().json(result))
    } else if body.method == "metashrew_preview" {
        // Ensure we have required params
        if body.params.len() < 4 {
            let error = JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [block_data, view_name, input_data, height]".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            };
            return Ok(HttpResponse::Ok().json(error));
        }

        let block_hex = match body.params[0].as_str() {
            Some(s) => s.to_string(),
            None => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: block_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        let view_name = match body.params[1].as_str() {
            Some(s) => s.to_string(),
            None => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: view_name must be a string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        let input_hex = match body.params[2].as_str() {
            Some(s) => s.to_string(),
            None => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: input_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        let height: u32 = match &body.params[3] {
            serde_json::Value::String(s) if s == "latest" => {
                fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
            }
            serde_json::Value::Number(n) => {
                let h = n.as_u64().unwrap_or(0) as u32;
                if h > height() {
                    fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
                } else {
                    h
                }
            }
            serde_json::Value::String(s) => {
                match s.parse::<u32>() {
                    Ok(h) => {
                        if h > height() {
                            fetch_and_set_height(&context.runtime.context.lock().unwrap().db).await?
                        } else {
                            h
                        }
                    }
                    Err(_) => {
                        let error = JsonRpcError {
                            id: body.id,
                            error: JsonRpcErrorObject {
                                code: -32602,
                                message: "Invalid params: height must be a number or 'latest'".to_string(),
                                data: None,
                            },
                            jsonrpc: "2.0".to_string(),
                        };
                        return Ok(HttpResponse::Ok().json(error));
                    }
                }
            }
            _ => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        let block_data = match hex::decode(block_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: format!("Invalid hex block data: {}", e),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                return Ok(HttpResponse::Ok().json(error));
            }
        };

        match context.runtime.preview(
            &block_data,
            view_name,
            &hex::decode(
                input_hex.trim_start_matches("0x"),
            ).map_err(|e| {
                error::ErrorBadRequest(format!("Invalid hex input: {}", e))
            })?,
            height,
        ) {
            Ok(res_string) => {
                let result = JsonRpcResult {
                    id: body.id,
                    result: String::from("0x") + hex::encode(res_string).as_str(),
                    jsonrpc: "2.0".to_string(),
                };
                Ok(HttpResponse::Ok().json(result))
            }
            Err(err) => {
                let error = JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32000,
                        message: err.to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                };
                Ok(HttpResponse::Ok().json(error))
            }
        }
    } else {
        let error = JsonRpcError {
            id: body.id,
            error: JsonRpcErrorObject {
                code: -32601,
                message: format!("Method '{}' not found", body.method),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        };
        Ok(HttpResponse::Ok().json(error))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    
    // Parse command line arguments (falls back to env vars via #[arg(env)])
    let args = RockshrewViewArgs::parse();
    
    if let Some(label) = args.label {
        set_label(label);
    }
    
    let program = File::open(&args.indexer).expect("Failed to open program file");
    let mut buf = BufReader::new(program);
    let mut bytes: Vec<u8> = vec![];
    let _ = buf.read_to_end(&mut bytes);
    let mut hasher = Sha3::v256();
    let mut output = [0; 32];
    hasher.update(bytes.as_slice());
    hasher.finalize(&mut output);
    info!("program hash: 0x{}", hex::encode(output));
    
    // Configure RocksDB options for optimal performance
    let mut opts = Options::default();
    opts.create_if_missing(false);
    opts.set_max_open_files(1000); // Reduced from 10000 to stay within system limits
    
    // Read-mostly optimizations
    opts.optimize_for_point_lookup(32 * 1024 * 1024);
    opts.increase_parallelism(4);
    opts.set_max_background_jobs(4);
    
    // Cache settings for reads
    opts.set_table_cache_num_shard_bits(4); // Reduced from 6 to lower file handle usage
    opts.set_max_file_opening_threads(8); // Reduced from 16
    
    // Secondary instance specific settings 
    opts.set_max_background_compactions(0);
    opts.set_disable_auto_compactions(true);
    
    // Create secondary path if it doesn't exist
    std::fs::create_dir_all(&args.secondary_path)?;
    
    // Setup periodic catch-up with primary
    let secondary_path = args.secondary_path.clone();
    let db_path = args.db_path.clone();
    
    let opts_clone = opts.clone();
    let catch_up_interval = std::time::Duration::from_secs(1);
    actix_web::rt::spawn(async move {
        let mut interval = actix_web::rt::time::interval(catch_up_interval);
        let mut retry_interval = actix_web::rt::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            match rocksdb::DB::open_as_secondary(&opts_clone, &db_path, &secondary_path) {
                Ok(db) => {
                    if let Err(e) = db.try_catch_up_with_primary() {
                        log::warn!("Error catching up with primary: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("Failed to open secondary DB: {}", e);
                    // Wait longer before retry on error
                    retry_interval.tick().await;
                }
            }
        }
    });

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
                    args.indexer.clone(),
                    RocksDBRuntimeAdapter::open_secondary(
                        args.db_path.clone(),
                        args.secondary_path.clone(),
                        opts.clone()
                    ).unwrap(),
                )
                .unwrap(),
            }))
            .service(jsonrpc_call)
    })
    .bind((args.host.as_str(), args.port))?
    .run()
    .await
}
