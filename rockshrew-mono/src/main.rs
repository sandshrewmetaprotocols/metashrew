use actix_cors::Cors;
use actix_web::error;
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use clap::Parser;
use env_logger;
use hex;
use log::{debug, info, error};
use rand::Rng;
use metashrew_runtime::{KeyValueStoreLike, MetashrewRuntime};
use num_cpus;
use reqwest::{Client, ClientBuilder, Response};
use rocksdb::Options;
use rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use serde::{Deserialize, Serialize};
use serde_json::{self, Number, Value};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::io::Read;
use tokio;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;
use url::Url as UrlParser;

// SSH tunneling support code
/// Configuration for SSH tunneling
#[derive(Debug, Clone)]
pub struct SshTunnelConfig {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub target_host: String,
    pub target_port: u16,
    pub local_port: u16,
}

impl SshTunnelConfig {
    /// Creates an SSH tunnel and returns the local port
    pub fn create_tunnel(&self) -> Result<SshTunnel> {
        info!("Creating SSH tunnel to {}:{} via {}@{}:{}",
            self.target_host, self.target_port,
            if self.ssh_user.is_empty() { "<from config>" } else { &self.ssh_user },
            self.ssh_host, self.ssh_port);
        
        // Build the SSH command
        let mut cmd = Command::new("ssh");
        
        // Add verbose flag for debugging
        cmd.arg("-v");
        
        // Add options to make the connection more robust
        cmd.arg("-o").arg("ServerAliveInterval=15")  // Send keepalive packets every 15 seconds
           .arg("-o").arg("ServerAliveCountMax=3")   // Disconnect after 3 failed keepalive packets
           .arg("-o").arg("ConnectTimeout=10")       // Timeout after 10 seconds if can't connect
           .arg("-o").arg("ExitOnForwardFailure=yes"); // Exit if port forwarding fails
        
        // Add user if specified
        if !self.ssh_user.is_empty() {
            cmd.arg(format!("{}@{}", self.ssh_user, self.ssh_host));
        } else {
            cmd.arg(&self.ssh_host);
        }
        
        // Add port if not default
        if self.ssh_port != 22 {
            cmd.arg("-p").arg(self.ssh_port.to_string());
        }
        
        debug!("SSH command: {:?}", cmd);
        
        // Add tunnel options
        cmd.arg("-N")  // Don't execute a remote command
           .arg("-T")  // Disable pseudo-terminal allocation
           .arg("-L")  // Local port forwarding
           .arg(format!("{}:{}:{}", self.local_port, self.target_host, self.target_port));
        
        // Start the SSH process with stderr captured for diagnostics
        let mut process = cmd.stdout(Stdio::null())
                         .stderr(Stdio::piped())
                         .spawn()?;
        
        // Wait a moment for the tunnel to establish
        std::thread::sleep(Duration::from_millis(2000));
        
        // Check if the process is still running
        match process.try_wait() {
            Ok(Some(status)) => {
                // Process has exited
                let mut stderr = process.stderr.take().unwrap();
                let mut error_output = String::new();
                stderr.read_to_string(&mut error_output).unwrap_or_else(|_| 0);
                return Err(anyhow!("SSH process exited with status {}: {}", status, error_output));
            },
            Ok(None) => {
                // Process is still running, which is good
                debug!("SSH process started successfully");
            },
            Err(e) => {
                return Err(anyhow!("Failed to check SSH process status: {}", e));
            }
        }
        
        // Try multiple times to connect to the local port
        let mut last_error = None;
        for attempt in 1..=5 {
            debug!("Attempting to connect to local port {} (attempt {})", self.local_port, attempt);
            match TcpStream::connect(format!("127.0.0.1:{}", self.local_port)) {
                Ok(_) => {
                    debug!("SSH tunnel established successfully on port {}", self.local_port);
                    return Ok(SshTunnel {
                        process: Arc::new(std::sync::Mutex::new(process)),
                        local_port: self.local_port,
                    });
                },
                Err(e) => {
                    debug!("Connection attempt {} failed: {}", attempt, e);
                    last_error = Some(e);
                    // Wait a bit before trying again
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
        
        // If we get here, all connection attempts failed
        let mut stderr = process.stderr.take().unwrap();
        let mut error_output = String::new();
        stderr.read_to_string(&mut error_output).unwrap_or_else(|_| 0);
        let _ = process.kill();
        
        match last_error {
            Some(e) => Err(anyhow!("Failed to establish SSH tunnel after 5 attempts: {} (SSH output: {})", e, error_output)),
            None => Err(anyhow!("Failed to establish SSH tunnel: unknown error (SSH output: {})", error_output))
        }
    }
}

/// Represents an active SSH tunnel
#[derive(Clone)]
pub struct SshTunnel {
    process: Arc<std::sync::Mutex<std::process::Child>>,
    pub local_port: u16,
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        // Only kill the process if this is the last reference
        if Arc::strong_count(&self.process) <= 1 {
            debug!("Closing SSH tunnel on port {}", self.local_port);
            let mut process = self.process.lock().unwrap();
            let _ = process.kill();
        }
    }
}

/// A response with an optional SSH tunnel that keeps the tunnel alive until the response is consumed
pub struct TunneledResponse {
    pub response: Response,
    // The tunnel is kept alive as long as this struct is alive
    pub _tunnel: Option<SshTunnel>,
}

impl TunneledResponse {
    pub fn new(response: Response, tunnel: Option<SshTunnel>) -> Self {
        Self {
            response,
            _tunnel: tunnel,
        }
    }
    
    /// Get the response bytes while keeping the tunnel alive
    pub async fn bytes(self) -> Result<Bytes> {
        // This consumes self, which keeps the tunnel alive until the bytes are read
        match self.response.bytes().await {
            Ok(bytes) => Ok(bytes),
            Err(e) => Err(anyhow!("Failed to get response bytes: {}", e)),
        }
    }
    
    /// Parse the response body as JSON while keeping the tunnel alive
    pub async fn json<T>(self) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        // This consumes self, which keeps the tunnel alive until the response is parsed
        match self.response.json::<T>().await {
            Ok(json) => Ok(json),
            Err(e) => Err(anyhow!("Failed to parse response as JSON: {}", e)),
        }
    }
    
    /// Get the response headers
    pub fn headers(&self) -> &reqwest::header::HeaderMap {
        self.response.headers()
    }
    
    /// Get the response status
    pub fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }
}

/// Find an available local port for the SSH tunnel
fn find_available_port() -> Result<u16> {
    // Try to bind to port 0, which lets the OS assign an available port
    let socket = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    Ok(port)
}

/// Read SSH config file to get host information
fn read_ssh_config(hostname: &str) -> Result<(String, u16, String)> {
    // Try to read the SSH config file
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
    let ssh_config_path = home_dir.join(".ssh").join("config");
    
    if !ssh_config_path.exists() {
        debug!("SSH config file not found at {:?}", ssh_config_path);
        return Ok((hostname.to_string(), 22, String::new()));
    }
    
    // Read the SSH config file
    let config_content = std::fs::read_to_string(&ssh_config_path)
        .map_err(|e| anyhow!("Failed to read SSH config file: {}", e))?;
    
    // Parse the SSH config file
    let mut in_host_section = false;
    let mut host_found = false;
    let mut actual_hostname = hostname.to_string();
    let mut port = 22;
    let mut user = String::new();
    
    for line in config_content.lines() {
        let line = line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        // Check if we're entering a Host section
        if line.to_lowercase().starts_with("host ") {
            let host_pattern = line[5..].trim();
            
            // Check if this Host section matches our hostname
            // Simple pattern matching for now - could be enhanced for wildcards
            if host_pattern == hostname {
                in_host_section = true;
                host_found = true;
                debug!("Found matching Host section for {}", hostname);
            } else {
                in_host_section = false;
            }
            continue;
        }
        
        // If we're in the matching Host section, parse the relevant options
        if in_host_section {
            if line.to_lowercase().starts_with("hostname ") {
                actual_hostname = line[9..].trim().to_string();
                debug!("Found HostName: {}", actual_hostname);
            } else if line.to_lowercase().starts_with("port ") {
                if let Ok(p) = line[5..].trim().parse::<u16>() {
                    port = p;
                    debug!("Found Port: {}", port);
                }
            } else if line.to_lowercase().starts_with("user ") {
                user = line[5..].trim().to_string();
                debug!("Found User: {}", user);
            }
        }
    }
    
    if host_found {
        debug!("Using SSH config for {}: hostname={}, port={}, user={}",
               hostname, actual_hostname, port, if user.is_empty() { "<none>" } else { &user });
        Ok((actual_hostname, port, user))
    } else {
        debug!("No matching Host section found for {}, using defaults", hostname);
        Ok((hostname.to_string(), 22, String::new()))
    }
}

/// Parses a daemon RPC URL and determines if SSH tunneling is needed
pub fn parse_daemon_rpc_url(url_str: &str) -> Result<(String, bool, Option<SshTunnelConfig>)> {
    // Check if the URL starts with ssh2+ prefix
    if url_str.starts_with("ssh2+http://") || url_str.starts_with("ssh2+https://") {
        let protocol = if url_str.starts_with("ssh2+https://") {
            "https"
        } else {
            "http"
        };
        
        // Remove the ssh2+ prefix
        let ssh_url = url_str.replace("ssh2+", "");
        let parsed_url = UrlParser::parse(&ssh_url)?;
        
        // Extract SSH connection details
        let ssh_host = parsed_url.host_str().ok_or_else(|| anyhow!("Missing SSH host"))?;
        let ssh_port = parsed_url.port().unwrap_or(22);
        let ssh_user = parsed_url.username();
        
        // Extract target details (after the path)
        let path = parsed_url.path();
        if path.is_empty() || path == "/" {
            return Err(anyhow!("Missing target host in path"));
        }
        
        // Remove leading slash and parse target
        let target = path.trim_start_matches('/');
        let target_parts: Vec<&str> = target.split(':').collect();
        
        let target_host = target_parts[0];
        let target_port = if target_parts.len() > 1 {
            target_parts[1].parse::<u16>()?
        } else {
            if protocol == "https" { 443 } else { 80 }
        };
        
        // Create the final target URL that will be used after tunneling
        let target_url = format!("{}://localhost:{}", protocol, target_port);
        
        // Check if we need to read SSH config
        let (actual_ssh_host, actual_ssh_port, config_ssh_user) = if ssh_user.is_empty() && ssh_port == 22 {
            // This looks like a hostname that might be in SSH config
            read_ssh_config(ssh_host)?
        } else {
            // Use the provided values
            (ssh_host.to_string(), ssh_port, ssh_user.to_string())
        };
        
        // Use config user if provided and no explicit user in URL
        let final_ssh_user = if ssh_user.is_empty() && !config_ssh_user.is_empty() {
            config_ssh_user
        } else {
            ssh_user.to_string()
        };
        
        // Create SSH tunnel config
        let tunnel_config = SshTunnelConfig {
            ssh_host: actual_ssh_host,
            ssh_port: actual_ssh_port,
            ssh_user: final_ssh_user,
            target_host: target_host.to_string(),
            target_port,
            local_port: find_available_port()?,
        };
        
        debug!("Parsed SSH tunnel config: {:?}", tunnel_config);
        return Ok((target_url, protocol == "https", Some(tunnel_config)));
    } else {
        // Regular URL without SSH tunneling
        let parsed_url = UrlParser::parse(url_str)?;
        let is_https = parsed_url.scheme() == "https";
        
        // Check if this is a localhost or IP address connection
        let host = parsed_url.host_str().ok_or_else(|| anyhow!("Missing host"))?;
        let is_localhost = host == "localhost"
            || host == "127.0.0.1"
            || host.starts_with("192.168.")
            || host.starts_with("10.")
            || host.starts_with("172.");
        
        // For HTTPS connections to localhost or IP addresses, we'll need to bypass SSL validation
        let bypass_ssl = is_https && is_localhost;
        
        debug!("Regular URL: {}, bypass_ssl: {}", url_str, bypass_ssl);
        return Ok((url_str.to_string(), bypass_ssl, None));
    }
}

/// Creates a reqwest Client with appropriate SSL configuration
pub fn create_http_client(bypass_ssl: bool) -> Result<Client> {
    let mut client_builder = ClientBuilder::new()
        .timeout(Duration::from_secs(60))         // Increased from 30 to 60 seconds
        .connect_timeout(Duration::from_secs(20)) // Increased from 10 to 20 seconds
        .pool_idle_timeout(Duration::from_secs(60)) // Keep connections alive longer
        .pool_max_idle_per_host(10);              // Increased from 5 to 10
    
    if bypass_ssl {
        debug!("Creating HTTP client with SSL validation disabled");
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }
    
    debug!("Creating HTTP client with increased timeouts (60s timeout, 20s connect)");
    Ok(client_builder.build()?)
}

/// Reads a file over SSH
pub async fn read_file_over_ssh(url_str: &str) -> Result<String> {
    // Parse the URL
    if !url_str.starts_with("sshfs://") {
        return Err(anyhow!("Not an sshfs URL"));
    }
    
    // Remove the sshfs:// prefix
    let ssh_url = url_str.replace("sshfs://", "");
    
    // Split into host and path parts
    let parts: Vec<&str> = ssh_url.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Invalid sshfs URL format, expected sshfs://host:path"));
    }
    
    let host = parts[0];
    let path = parts[1];
    
    // Build the SSH command to read the file
    let mut cmd = Command::new("ssh");
    
    // Check if host contains @ for username
    if host.contains('@') {
        // Direct user@host format
        cmd.arg(host);
    } else {
        // Might be a hostname from SSH config
        let (actual_host, port, user) = read_ssh_config(host)?;
        
        // Add user if specified in config
        if !user.is_empty() {
            cmd.arg(format!("{}@{}", user, actual_host));
        } else {
            cmd.arg(actual_host);
        }
        
        // Add port if not default
        if port != 22 {
            cmd.arg("-p").arg(port.to_string());
        }
    }
    
    // Add the command to read the file
    cmd.arg("cat").arg(path);
    
    // Execute the command
    debug!("Reading file over SSH: {}:{}", host, path);
    let output = cmd.output()?;
    
    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to read file over SSH: {}", error_message));
    }
    
    // Convert the output to a string
    let content = String::from_utf8(output.stdout)?;
    debug!("Successfully read file over SSH (length: {})", content.len());
    
    Ok(content.trim().to_string())
}

/// Makes an HTTP request through an SSH tunnel if needed
pub async fn make_request_with_tunnel(
    url: &str,
    body: String,
    auth: Option<String>,
    tunnel_config: Option<SshTunnelConfig>,
    bypass_ssl: bool
) -> Result<TunneledResponse> {
    // Create HTTP client with appropriate SSL configuration
    let client = create_http_client(bypass_ssl)?;
    
    // If we have tunnel config, create the tunnel with retry logic
    let tunnel = match tunnel_config {
        Some(config) => {
            // Try to create the tunnel up to 3 times
            let mut last_error = None;
            let mut tunnel = None;
            
            for attempt in 1..=3 {
                debug!("Attempting to create SSH tunnel (attempt {})", attempt);
                match config.create_tunnel() {
                    Ok(t) => {
                        info!("SSH tunnel created successfully on port {}", t.local_port);
                        tunnel = Some(t);
                        break;
                    },
                    Err(e) => {
                        error!("Failed to create SSH tunnel (attempt {}): {}", attempt, e);
                        last_error = Some(e);
                        
                        // Wait before retrying
                        if attempt < 3 {
                            std::thread::sleep(Duration::from_secs(2));
                        }
                    }
                }
            }
            
            // If all attempts failed, return the last error
            if tunnel.is_none() && last_error.is_some() {
                return Err(anyhow!("Failed to create SSH tunnel after 3 attempts: {}",
                                  last_error.unwrap()));
            }
            
            tunnel
        },
        None => None,
    };
    
    // Determine the final URL to use
    let final_url = match &tunnel {
        Some(tunnel) => {
            // Parse the original URL
            let mut parsed_url = UrlParser::parse(url)?;
            
            // Update the host and port to use the local tunnel
            parsed_url.set_host(Some("localhost")).map_err(|_| anyhow!("Failed to set host"))?;
            parsed_url.set_port(Some(tunnel.local_port)).map_err(|_| anyhow!("Failed to set port"))?;
            
            parsed_url.to_string()
        },
        None => url.to_string(),
    };
    
    // Add authentication if provided
    let final_url = if let Some(auth_str) = auth {
        // Check if auth is an sshfs URL
        if auth_str.starts_with("sshfs://") {
            // Read the auth from the file over SSH
            let cookie_content = read_file_over_ssh(&auth_str).await?;
            
            // Parse the cookie content (expected format: username:password)
            let mut parsed_url = UrlParser::parse(&final_url)?;
            
            if cookie_content.contains(':') {
                let (username, password) = cookie_content.split_once(':')
                    .ok_or_else(|| anyhow!("Invalid cookie format, expected username:password"))?;
                
                parsed_url.set_username(username)
                    .map_err(|_| anyhow!("Failed to set username"))?;
                parsed_url.set_password(Some(password))
                    .map_err(|_| anyhow!("Failed to set password"))?;
            } else {
                // If no colon, assume the whole content is the password for bitcoinrpc user
                parsed_url.set_username("bitcoinrpc")
                    .map_err(|_| anyhow!("Failed to set username"))?;
                parsed_url.set_password(Some(&cookie_content))
                    .map_err(|_| anyhow!("Failed to set password"))?;
            }
            
            parsed_url.to_string()
        } else {
            // Regular auth string
            let mut parsed_url = UrlParser::parse(&final_url)?;
            let (username, password) = auth_str.split_once(':')
                .ok_or_else(|| anyhow!("Invalid auth format, expected username:password"))?;
            
            parsed_url.set_username(username)
                .map_err(|_| anyhow!("Failed to set username"))?;
            parsed_url.set_password(Some(password))
                .map_err(|_| anyhow!("Failed to set password"))?;
            
            parsed_url.to_string()
        }
    } else {
        final_url
    };
    
    // Make the request
    debug!("Making request to {}", final_url);
    let response = client
        .post(&final_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")  // Explicitly request JSON response
        .body(body)
        .send()
        .await?;
    
    // Check response status before returning
    if !response.status().is_success() {
        let status = response.status();
        debug!("Received error status: {}", status);
        
        // Try to get error details from response
        let error_text = match response.text().await {
            Ok(text) => {
                if text.len() < 1000 {
                    format!("{}", text)
                } else {
                    format!("{} (truncated)", &text[..1000])
                }
            },
            Err(_) => "Could not read error response body".to_string()
        };
        
        return Err(anyhow!("HTTP error response: {} - {}", status, error_text));
    }
    
    debug!("Received successful response with status: {}", response.status());
    Ok(TunneledResponse::new(response, tunnel))
}

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";
use std::sync::atomic::{AtomicU32, Ordering};
static CURRENT_HEIGHT: AtomicU32 = AtomicU32::new(0);

// Block processing result for the pipeline
#[derive(Debug)]
enum BlockResult {
    Success(u32),  // Block height that was successfully processed
    Error(u32, anyhow::Error),  // Block height and error
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long)]
    indexer: String,
    #[arg(long)]
    db_path: String,
    #[arg(long)]
    start_block: Option<u32>,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long)]
    label: Option<String>,
    #[arg(long)]
    exit_at: Option<u32>,
    // JSON-RPC server args
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,
    #[arg(long, env = "PORT", default_value_t = 8080)]
    port: u16,
    #[arg(long, help = "CORS allowed origins (e.g., '*' for all origins, or specific domains)")]
    cors: Option<String>,
    // Pipeline configuration
    #[arg(long, help = "Size of the processing pipeline (default: auto-determined based on CPU cores)")]
    pipeline_size: Option<usize>,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
}

#[derive(Serialize, Deserialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
    params: Vec<Value>,
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

#[derive(Debug)]
struct IndexerError(anyhow::Error);

impl std::fmt::Display for IndexerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<anyhow::Error> for IndexerError {
    fn from(err: anyhow::Error) -> Self {
        IndexerError(err)
    }
}

impl error::ResponseError for IndexerError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::Ok().json(JsonRpcError {
            id: 0, // Generic ID since we lost context
            error: JsonRpcErrorObject {
                code: -32000,
                message: self.0.to_string(),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        })
    }
}
struct IndexerState {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    args: Arc<Args>,
    start_block: u32,
    fetcher_thread_id_tx: Option<tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>>,
    processor_thread_id_tx: Option<tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>>,
    fetcher_thread_id: std::sync::Mutex<Option<std::thread::ThreadId>>,
    processor_thread_id: std::sync::Mutex<Option<std::thread::ThreadId>>,
    // SSH tunnel configuration
    rpc_url: String,
    bypass_ssl: bool,
    tunnel_config: Option<SshTunnelConfig>,
    // Active SSH tunnel for reuse
    active_tunnel: std::sync::Mutex<Option<SshTunnel>>,
    // Cached auth credentials
    cached_auth: std::sync::Mutex<Option<String>>,
    // Last auth error timestamp
    last_auth_error: std::sync::Mutex<Option<SystemTime>>,
}

impl IndexerState {
    async fn post_once(&self, body: String) -> Result<TunneledResponse, reqwest::Error> {
        // Get cached auth or fetch new auth if needed
        let auth = if let Some(auth_str) = &self.args.auth {
            if auth_str.starts_with("sshfs://") {
                // Check if we have cached auth and if it's still valid
                let should_refresh = {
                    let cached_auth = self.cached_auth.lock().unwrap();
                    let last_error = self.last_auth_error.lock().unwrap();
                    
                    // Refresh if no cached auth or if we had an auth error recently
                    cached_auth.is_none() ||
                    (last_error.is_some() && last_error.unwrap().elapsed().unwrap_or_default() < Duration::from_secs(60))
                };
                
                if should_refresh {
                    debug!("Fetching fresh auth credentials from {}", auth_str);
                    // Read the auth from the file over SSH
                    match read_file_over_ssh(auth_str).await {
                        Ok(content) => {
                            // Cache the auth credentials
                            let mut cached_auth = self.cached_auth.lock().unwrap();
                            *cached_auth = Some(content.clone());
                            Some(content)
                        },
                        Err(e) => {
                            error!("Failed to read auth file over SSH: {}", e);
                            // Return a generic error
                            let client = reqwest::Client::new();
                            let url = "http://invalid-url-that-will-fail";
                            return match client.get(url).send().await {
                                Ok(_) => unreachable!(),
                                Err(err) => Err(err),
                            };
                        }
                    }
                } else {
                    // Use cached auth
                    let cached_auth = self.cached_auth.lock().unwrap();
                    cached_auth.clone()
                }
            } else {
                // Regular auth string
                Some(auth_str.clone())
            }
        } else {
            None
        };
        
        // Check if we have an active tunnel we can reuse
        let reuse_tunnel = if self.tunnel_config.is_some() {
            let tunnel_mutex = self.active_tunnel.lock().unwrap();
            tunnel_mutex.is_some()
        } else {
            false
        };
        
        if reuse_tunnel {
            debug!("Reusing existing SSH tunnel");
            // Use the existing tunnel with a new request
            let client = match create_http_client(self.bypass_ssl) {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to create HTTP client: {}", e);
                    // Create a dummy request that will fail
                    let client = reqwest::Client::new();
                    let url = "http://invalid-url-that-will-fail";
                    return match client.get(url).send().await {
                        Ok(_) => unreachable!(),
                        Err(err) => Err(err),
                    };
                }
            };
            
            // Get the tunnel port
            let tunnel_port = {
                let tunnel_mutex = self.active_tunnel.lock().unwrap();
                tunnel_mutex.as_ref().unwrap().local_port
            };
            
            // Create the URL with the tunnel port
            let parsed_url = match UrlParser::parse(&self.rpc_url) {
                Ok(mut url) => {
                    if let Err(e) = url.set_host(Some("localhost")) {
                        error!("Failed to set host: {}", e);
                        // Create a dummy request that will fail
                        let client = reqwest::Client::new();
                        let url = "http://invalid-url-that-will-fail";
                        return match client.get(url).send().await {
                            Ok(_) => unreachable!(),
                            Err(err) => Err(err),
                        };
                    }
                    
                    if let Err(e) = url.set_port(Some(tunnel_port)) {
                        error!("Failed to set port: {:?}", e);
                        // Create a dummy request that will fail
                        let client = reqwest::Client::new();
                        let url = "http://invalid-url-that-will-fail";
                        return match client.get(url).send().await {
                            Ok(_) => unreachable!(),
                            Err(err) => Err(err),
                        };
                    }
                    
                    url
                },
                Err(e) => {
                    error!("Failed to parse URL: {}", e);
                    // Create a dummy request that will fail
                    let client = reqwest::Client::new();
                    let url = "http://invalid-url-that-will-fail";
                    return match client.get(url).send().await {
                        Ok(_) => unreachable!(),
                        Err(err) => Err(err),
                    };
                }
            };
            
            let final_url = parsed_url.to_string();
            
            // Add authentication if provided
            let final_url = if let Some(auth_str) = auth {
                match UrlParser::parse(&final_url) {
                    Ok(mut parsed_url) => {
                        if auth_str.contains(':') {
                            match auth_str.split_once(':') {
                                Some((username, password)) => {
                                    if let Err(e) = parsed_url.set_username(username) {
                                        error!("Failed to set username: {:?}", e);
                                        // Create a dummy request that will fail
                                        let client = reqwest::Client::new();
                                        let url = "http://invalid-url-that-will-fail";
                                        return match client.get(url).send().await {
                                            Ok(_) => unreachable!(),
                                            Err(err) => Err(err),
                                        };
                                    }
                                    
                                    if let Err(e) = parsed_url.set_password(Some(password)) {
                                        error!("Failed to set password: {:?}", e);
                                        // Create a dummy request that will fail
                                        let client = reqwest::Client::new();
                                        let url = "http://invalid-url-that-will-fail";
                                        return match client.get(url).send().await {
                                            Ok(_) => unreachable!(),
                                            Err(err) => Err(err),
                                        };
                                    }
                                },
                                None => {
                                    error!("Invalid auth format");
                                    // Create a dummy request that will fail
                                    let client = reqwest::Client::new();
                                    let url = "http://invalid-url-that-will-fail";
                                    return match client.get(url).send().await {
                                        Ok(_) => unreachable!(),
                                        Err(err) => Err(err),
                                    };
                                }
                            }
                        } else {
                            // If no colon, assume the whole content is the password for bitcoinrpc user
                            if let Err(e) = parsed_url.set_username("bitcoinrpc") {
                                error!("Failed to set username: {:?}", e);
                                // Create a dummy request that will fail
                                let client = reqwest::Client::new();
                                let url = "http://invalid-url-that-will-fail";
                                return match client.get(url).send().await {
                                    Ok(_) => unreachable!(),
                                    Err(err) => Err(err),
                                };
                            }
                            
                            if let Err(e) = parsed_url.set_password(Some(&auth_str)) {
                                error!("Failed to set password: {:?}", e);
                                // Create a dummy request that will fail
                                let client = reqwest::Client::new();
                                let url = "http://invalid-url-that-will-fail";
                                return match client.get(url).send().await {
                                    Ok(_) => unreachable!(),
                                    Err(err) => Err(err),
                                };
                            }
                        }
                        
                        parsed_url.to_string()
                    },
                    Err(e) => {
                        error!("Failed to parse URL: {}", e);
                        // Create a dummy request that will fail
                        let client = reqwest::Client::new();
                        let url = "http://invalid-url-that-will-fail";
                        return match client.get(url).send().await {
                            Ok(_) => unreachable!(),
                            Err(err) => Err(err),
                        };
                    }
                }
            } else {
                final_url
            };
            
            // Make the request
            debug!("Making request to {} with reused tunnel", final_url);
            let response = client
                .post(&final_url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .body(body)
                .send()
                .await?;
            
            // Check for auth errors
            if response.status() == reqwest::StatusCode::UNAUTHORIZED ||
               response.status() == reqwest::StatusCode::FORBIDDEN {
                // Mark that we had an auth error
                let mut last_error = self.last_auth_error.lock().unwrap();
                *last_error = Some(SystemTime::now());
            }
            
            // Return the response without a tunnel (since we're keeping it alive separately)
            Ok(TunneledResponse::new(response, None))
        } else {
            // Create a new tunnel
            match make_request_with_tunnel(
                &self.rpc_url,
                body,
                auth,
                self.tunnel_config.clone(),
                self.bypass_ssl
            ).await {
                Ok(tunneled_response) => {
                    // If this is a tunneled response with a tunnel, store it for reuse
                    if let Some(tunnel) = tunneled_response._tunnel.clone() {
                        if self.tunnel_config.is_some() {
                            debug!("Storing SSH tunnel for reuse on port {}", tunnel.local_port);
                            let mut active_tunnel = self.active_tunnel.lock().unwrap();
                            *active_tunnel = Some(tunnel);
                            
                            // Return the response without the tunnel (since we're keeping it alive separately)
                            Ok(TunneledResponse::new(tunneled_response.response, None))
                        } else {
                            Ok(tunneled_response)
                        }
                    } else {
                        Ok(tunneled_response)
                    }
                },
                Err(e) => {
                    // Log the error and convert to a string error
                    error!("Request failed: {}", e);
                    
                    // Create a dummy request that will fail
                    let client = reqwest::Client::new();
                    let url = "http://invalid-url-that-will-fail";
                    
                    // Return the error from the failed request
                    match client.get(url).send().await {
                        Ok(_) => unreachable!(),
                        Err(err) => Err(err),
                    }
                },
            }
        }
    }
#[allow(unused_assignments)]
async fn post(&self, body: String) -> Result<TunneledResponse> {
    let mut retry_delay = Duration::from_millis(100);
    let max_delay = Duration::from_secs(30);
    let max_retries = 10;
    let _response: Option<TunneledResponse> = None;
    
    for attempt in 0..=max_retries {
        match self.post_once(body.clone()).await {
            Ok(tunneled_response) => {
                return Ok(tunneled_response);
            },
            Err(e) => {
                if attempt == max_retries {
                    return Err(anyhow!("Max retries exceeded: {}", e));
                }
                
                // Calculate exponential backoff with jitter
                let jitter = rand::thread_rng().gen_range(0..=100) as u64;
                retry_delay = std::cmp::min(
                    max_delay,
                    retry_delay * 2 + Duration::from_millis(jitter)
                );
                
                debug!("Request failed (attempt {}): {}, retrying in {:?}",
                       attempt + 1, e, retry_delay);
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
    
    Err(anyhow!("Unreachable: max retries exceeded"))
    }

    async fn fetch_blockcount(&self) -> Result<u32> {
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockcount"),
                params: vec![],
            })?)
            .await?;

        let result: Value = tunneled_response.json().await?;
        Ok(result["result"]
            .as_u64()
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))? as u32)
    }

    async fn query_height(&self) -> Result<u32> {
        let (db, start_block) = {
            let runtime = self.runtime.read().await;
            let context = runtime.context.lock().unwrap();
            (context.db.db.clone(), self.start_block)
        };
        query_height(db, start_block).await
    }

    async fn best_height(&self, block_number: u32) -> Result<u32> {
        let mut best: u32 = block_number;
        let tip = self.fetch_blockcount().await?;

        if best >= tip - std::cmp::min(6, tip) {
            loop {
                if best == 0 {
                    break;
                }
                
                // Get local blockhash with better error handling
                let blockhash = match self.get_blockhash(best).await {
                    Some(hash) => hash,
                    None => {
                        // If we can't get the local blockhash, try to get it from the remote
                        debug!("Local blockhash not found for block {}, fetching from remote", best);
                        let remote_hash = self.fetch_blockhash(best).await?;
                        
                        // Store the remote hash locally for future reference
                        let runtime = self.runtime.write().await;
                        if let Ok(mut context) = runtime.context.lock() {
                            let key = (String::from(HEIGHT_TO_HASH) + &best.to_string()).into_bytes();
                            if let Err(e) = context.db.put(&key, &remote_hash) {
                                debug!("Failed to store blockhash for block {}: {}", best, e);
                            }
                        }
                        
                        remote_hash
                    }
                };
                
                let remote_blockhash = self.fetch_blockhash(best).await?;
                if blockhash == remote_blockhash {
                    break;
                } else {
                    best = best - 1;
                }
            }
        }
        Ok(best)
    }

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        let key = (String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes();
        let runtime = match self.runtime.read().await {
            runtime => runtime,
        };
        
        let mut context = match runtime.context.lock() {
            Ok(context) => context,
            Err(e) => {
                error!("Failed to lock context: {}", e);
                return None;
            }
        };
        
        match context.db.get(&key) {
            Ok(result) => result,
            Err(e) => {
                error!("Database error when retrieving blockhash for block {}: {}", block_number, e);
                None
            }
        }
    }

    async fn fetch_blockhash(&self, block_number: u32) -> Result<Vec<u8>> {
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockhash"),
                params: vec![Value::Number(Number::from(block_number))],
            })?)
            .await?;

        // Get the response bytes while keeping the tunnel alive
        let bytes_data = tunneled_response.bytes().await?;
        
        // Parse the JSON response
        let result: Value = serde_json::from_slice(&bytes_data)?;
        let blockhash = result["result"]
            .as_str()
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))?;
        Ok(hex::decode(blockhash)?)
    }

    async fn pull_block(&self, block_number: u32) -> Result<Vec<u8>> {
        loop {
            let count = self.fetch_blockcount().await?;
            if block_number > count {
                tokio::time::sleep(Duration::from_millis(3000)).await;
            } else {
                break;
            }
        }
        let blockhash = self.fetch_blockhash(block_number).await?;

        let runtime = self.runtime.write().await;
        runtime.context.lock().unwrap().db.put(
            &(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes(),
            &blockhash,
        )?;

        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblock"),
                params: vec![
                    Value::String(hex::encode(&blockhash)),
                    Value::Number(Number::from(0)),
                ],
            })?)
            .await?;

        // Get the content type before consuming the response
        let content_type = tunneled_response.headers().get("content-type")
            .map_or("unknown".to_string(), |v| v.to_str().unwrap_or("invalid").to_string());
        
        info!("Response content type: {}", content_type);
        
        // Get the raw bytes from the response while keeping the tunnel alive
        let bytes_data = match tunneled_response.bytes().await {
            Ok(b) => {
                info!("Successfully received response bytes: {} bytes", b.len());
                b
            },
            Err(e) => {
                error!("Failed to get response bytes: {}", e);
                error!("Error details: {:?}", e);
                return Err(anyhow!("Failed to get response bytes: {}", e));
            }
        };
        
        // Log the raw bytes for debugging
        if bytes_data.len() < 100 {
            info!("Full response (hex): {}", hex::encode(&bytes_data));
        } else {
            info!("Response start (hex): {}", hex::encode(&bytes_data[..100]));
        }
        
        // Try to convert to string for logging
        match std::str::from_utf8(&bytes_data) {
            Ok(text) => {
                if text.len() < 1000 {
                    info!("Response as text: {}", text);
                } else {
                    info!("Response as text (truncated): {}", &text[..1000]);
                }
            },
            Err(_) => {
                info!("Response is not valid UTF-8");
            }
        }
        
        // Parse the JSON response
        let result: Value = match serde_json::from_slice(&bytes_data) {
            Ok(json) => {
                info!("Successfully parsed JSON response");
                json
            },
            Err(e) => {
                error!("Failed to parse JSON response: {}", e);
                error!("Response type: {}", content_type);
                return Err(anyhow!("Failed to parse JSON response: {}", e));
            }
        };
        
        // Extract the block hex
        let block_hex = match result.get("result") {
            Some(result_value) => {
                match result_value.as_str() {
                    Some(hex) => hex,
                    None => {
                        error!("Result is not a string: {:?}", result_value);
                        return Err(anyhow!("Result is not a string: {:?}", result_value));
                    }
                }
            },
            None => {
                // Check if there's an error in the response
                if let Some(error) = result.get("error") {
                    error!("RPC error response: {:?}", error);
                    return Err(anyhow!("RPC error response: {:?}", error));
                }
                
                error!("Missing 'result' field in JSON-RPC response: {:?}", result);
                return Err(anyhow!("Missing 'result' field in JSON-RPC response"));
            }
        };
        
        // Decode the hex
        match hex::decode(block_hex) {
            Ok(bytes) => {
                debug!("Successfully decoded block data: {} bytes", bytes.len());
                Ok(bytes)
            },
            Err(e) => {
                error!("Failed to decode hex: {}", e);
                error!("Hex string length: {}", block_hex.len());
                error!("Hex string prefix: {}", if block_hex.len() < 100 { block_hex } else { &block_hex[..100] });
                Err(anyhow!("Failed to decode hex: {}", e))
            }
        }
    }

    // Process a single block
    async fn process_block(&self, height: u32, block_data: Vec<u8>) -> Result<()> {
        // Get a lock on the runtime with better error handling
        let mut runtime = match self.runtime.write().await {
            runtime => runtime,
        };
        
        // Set block data with better error handling
        match runtime.context.lock() {
            Ok(mut context) => {
                context.block = block_data;
                context.height = height;
                context.db.set_height(height);
            },
            Err(e) => {
                return Err(anyhow!("Failed to lock context: {}", e));
            }
        }
        
        // Check if memory usage is approaching the limit and refresh if needed
        if self.should_refresh_memory(&mut runtime, height) {
            match runtime.refresh_memory() {
                Ok(_) => debug!("Successfully refreshed memory preemptively for block {}", height),
                Err(e) => {
                    error!("Failed to preemptively refresh memory: {}", e);
                    // Continue with execution even if preemptive refresh fails
                }
            }
        }
        
        // Execute the runtime with better error handling
        match runtime.run() {
            Ok(_) => {
                debug!("Successfully processed block {}", height);
                
                // Store the blockhash for this height to ensure it's available for future queries
                if let Ok(mut context) = runtime.context.lock() {
                    if let Ok(Some(_blockhash)) = context.db.get(&format!("{}{}",
                        HEIGHT_TO_HASH, height).into_bytes()) {
                        debug!("Verified blockhash is stored for block {}", height);
                    } else {
                        debug!("Blockhash not found for block {}, will be fetched if needed", height);
                    }
                }
                
                Ok(())
            },
            Err(e) => {
                // Log detailed memory stats when runtime execution fails
                let memory_stats = self.get_memory_stats(&mut runtime);
                error!("CRITICAL: Runtime execution failed for block {}: {}", height, e);
                error!("Memory stats at failure: {}", memory_stats);
                
                // Crash the process to force a restart and make the issue more visible
                panic!("Runtime execution failed with memory stats: {}", memory_stats);
                
                // The code below is unreachable due to the panic, but kept for reference
                /*
                // Try to refresh memory with better error handling
                match runtime.refresh_memory() {
                    Ok(_) => {
                        // Try running again after memory refresh
                        match runtime.run() {
                            Ok(_) => {
                                debug!("Successfully processed block {} after memory refresh", height);
                                Ok(())
                            },
                            Err(run_err) => {
                                error!("Runtime execution failed after memory refresh: {}", run_err);
                                Err(anyhow!(run_err))
                            }
                        }
                    },
                    Err(refresh_err) => {
                        error!("Memory refresh failed: {}", refresh_err);
                        Err(anyhow!(refresh_err))
                    }
                }
                */
            }
        }
    }

    // Improvement 5: Parallel processing with pipeline
    async fn run_pipeline(&mut self) -> Result<()> {
        let mut height: u32 = self.query_height().await?;
        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
        // Determine optimal pipeline size based on CPU cores if not specified
        let pipeline_size = match self.args.pipeline_size {
            Some(size) => size,
            None => {
                let available_cpus = num_cpus::get();
                // Use approximately 1/2 of available cores for pipeline size, with reasonable min/max
                let auto_size = std::cmp::min(
                    std::cmp::max(5, available_cpus / 2),
                    16  // Cap at a reasonable maximum
                );
                info!("Auto-configuring pipeline size to {} based on {} available CPU cores", auto_size, available_cpus);
                auto_size
            }
        };
        
        // Create channels for the pipeline
        let (block_sender, mut block_receiver) = mpsc::channel::<(u32, Vec<u8>)>(pipeline_size);
        let (result_sender, mut result_receiver) = mpsc::channel::<BlockResult>(pipeline_size);
        
        // Spawn block fetcher task on dedicated thread
        let fetcher_handle = {
            let args = self.args.clone();
            let indexer = self.clone();
            let result_sender_clone = result_sender.clone();
            let block_sender_clone = block_sender.clone();
            
            // Spawn a task for the block fetcher
            tokio::spawn(async move {
                    // Register this thread as the fetcher thread
                    indexer.register_current_thread_as_fetcher();
                    info!("Block fetcher task started on thread {:?}", std::thread::current().id());
                let mut current_height = height;
                
                loop {
                    // Check if we should exit
                    if let Some(exit_at) = args.exit_at {
                        if current_height >= exit_at {
                            info!("Fetcher reached exit-at block {}, shutting down", exit_at);
                            break;
                        }
                    }
                    
                    // Find the best height considering potential reorgs
                    let best_height = match indexer.best_height(current_height).await {
                        Ok(h) => h,
                        Err(e) => {
                            error!("Failed to determine best height: {}", e);
                            // Send error result and continue
                            if result_sender_clone.send(BlockResult::Error(current_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    
                    // Fetch the block
                    match indexer.pull_block(best_height).await {
                        Ok(block_data) => {
                            debug!("Fetched block {} ({})", best_height, block_data.len());
                            // Send block to processor
                            if block_sender_clone.send((best_height, block_data)).await.is_err() {
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Failed to fetch block {}: {}", best_height, e);
                            error!("Error details: {:?}", e);
                            
                            // Log the error chain if available
                            if let Some(cause) = e.root_cause().source() {
                                error!("Root cause: {}", cause);
                            }
                            
                            // Send error result
                            if result_sender_clone.send(BlockResult::Error(best_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    }
                    
                    current_height = best_height + 1;
                }
                
                debug!("Block fetcher task completed");
            })
        };
        
        // Spawn block processor task on dedicated thread
        let processor_handle = {
            let indexer = self.clone();
            let result_sender_clone = result_sender.clone();
            
            // Spawn a task for the block processor
            tokio::spawn(async move {
                    // Register this thread as the processor thread
                    indexer.register_current_thread_as_processor();
                    info!("Block processor task started on thread {:?}", std::thread::current().id());
                while let Some((block_height, block_data)) = block_receiver.recv().await {
                    debug!("Processing block {} ({})", block_height, block_data.len());
                    
                    let result = match indexer.process_block(block_height, block_data).await {
                        Ok(_) => BlockResult::Success(block_height),
                        Err(e) => BlockResult::Error(block_height, e),
                    };
                    
                    // Send result
                    if result_sender_clone.send(result).await.is_err() {
                        break;
                    }
                }
                
                debug!("Block processor task completed");
            })
        };
        
        // Main loop to handle results
        while let Some(result) = result_receiver.recv().await {
            match result {
                BlockResult::Success(processed_height) => {
                    debug!("Successfully processed block {}", processed_height);
                    height = processed_height + 1;
                    CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                },
                BlockResult::Error(failed_height, error) => {
                    error!("Failed to process block {}: {}", failed_height, error);
                    error!("Error details: {:?}", error);
                    
                    // Log the error chain if available
                    if let Some(cause) = error.root_cause().source() {
                        error!("Root cause: {}", cause);
                    }
                    
                    // We could implement more sophisticated error handling here
                    // For now, just wait and continue
                    sleep(Duration::from_secs(5)).await;
                }
            }
            
            // Check if we should exit
            if let Some(exit_at) = self.args.exit_at {
                if height > exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
                    break;
                }
            }
        }
        
        // Clean up
        drop(block_sender);
        drop(result_sender);
        
        // Wait for tasks to complete
        let (_fetcher_result, _processor_result) = tokio::join!(fetcher_handle, processor_handle);
        
        Ok(())
    }

    #[allow(dead_code)]
    async fn run(&mut self) -> Result<()> {
        let mut height: u32 = self.query_height().await?;

        loop {
            if let Some(exit_at) = self.args.exit_at {
                if height >= exit_at {
                    info!(
                        "Reached exit-at block {}, shutting down gracefully",
                        exit_at
                    );
                    return Ok(());
                }
            }

            let best: u32 = self.best_height(height).await.unwrap_or(height);
            let block_data = self.pull_block(best).await?;

            let mut runtime = self.runtime.write().await;
            {
                let mut context = runtime.context.lock().unwrap();
                context.block = block_data;
                context.height = best;
                context.db.set_height(best);
            }

            match runtime.run() {
                Ok(_) => {},
                Err(e) => {
                    info!("Runtime execution failed: {}, refreshing memory and retrying", e);
                    runtime.refresh_memory().map_err(|refresh_err| {
                        error!("Memory refresh failed: {}", refresh_err);
                        refresh_err
                    })?;
                    
                    runtime.run().map_err(|run_err| {
                        error!("Runtime execution failed after memory refresh: {}", run_err);
                        run_err
                    })?;
                }
            }

            height = best + 1;
            CURRENT_HEIGHT.store(height, Ordering::SeqCst);
        }
    }
}

// Allow cloning for use in async tasks
impl Clone for IndexerState {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            args: self.args.clone(),
            start_block: self.start_block,
            fetcher_thread_id_tx: self.fetcher_thread_id_tx.clone(),
            processor_thread_id_tx: self.processor_thread_id_tx.clone(),
            fetcher_thread_id: std::sync::Mutex::new(*self.fetcher_thread_id.lock().unwrap()),
            processor_thread_id: std::sync::Mutex::new(*self.processor_thread_id.lock().unwrap()),
            // SSH tunnel configuration
            rpc_url: self.rpc_url.clone(),
            bypass_ssl: self.bypass_ssl,
            tunnel_config: self.tunnel_config.clone(),
            // Clone the new fields
            active_tunnel: std::sync::Mutex::new(None), // Don't clone the active tunnel, create a new one if needed
            cached_auth: std::sync::Mutex::new(self.cached_auth.lock().unwrap().clone()),
            last_auth_error: std::sync::Mutex::new(self.last_auth_error.lock().unwrap().clone()),
        }
    }
}

// Add methods to set and get thread ID senders
impl IndexerState {
    // Helper method to get detailed memory statistics as a string
    fn get_memory_stats(&self, runtime: &mut MetashrewRuntime<RocksDBRuntimeAdapter>) -> String {
        if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
            let memory_size = memory.data_size(&mut runtime.wasmstore);
            let memory_size_gb = memory_size as f64 / 1_073_741_824.0;
            let memory_size_mb = memory_size as f64 / 1_048_576.0;
            
            // Get additional memory information if available
            let memory_pages = memory.size(&mut runtime.wasmstore);
            
            format!(
                "Memory size: {} bytes ({:.2} GB, {:.2} MB), Pages: {}, Usage: {:.2}%",
                memory_size,
                memory_size_gb,
                memory_size_mb,
                memory_pages,
                (memory_size as f64 / 4_294_967_296.0) * 100.0 // Percentage of 4GB limit
            )
        } else {
            "Could not access memory instance".to_string()
        }
    }
    
    // Helper method to check if memory needs to be refreshed based on its size
    fn should_refresh_memory(&self, runtime: &mut MetashrewRuntime<RocksDBRuntimeAdapter>, height: u32) -> bool {
        // Get the memory instance
        if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
            // Get the memory size in bytes
            let memory_size = memory.data_size(&mut runtime.wasmstore);
            
            // 1.75GB in bytes = 1.75 * 1024 * 1024 * 1024
            let threshold_gb = 1.75;
            let threshold_bytes = (threshold_gb * 1024.0 * 1024.0 * 1024.0) as usize;
            
            // Get detailed memory stats for logging
            let memory_stats = self.get_memory_stats(runtime);
            
            // Check if memory size is approaching the limit
            if memory_size >= threshold_bytes {
                info!("Memory usage approaching threshold of {:.2}GB for block {}: {}", threshold_gb, height, memory_stats);
                info!("Preemptively refreshing memory to avoid OOM errors");
                return true;
            } else if height % 1000 == 0 {
                // Log memory stats periodically for monitoring
                info!("Memory stats at block {}: {}", height, memory_stats);
            }
        } else {
            debug!("Could not get memory instance for block {}", height);
        }
        
        false
    }
    fn set_thread_id_senders(
        &mut self,
        fetcher_tx: tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>,
        processor_tx: tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>
    ) {
        self.fetcher_thread_id_tx = Some(fetcher_tx);
        self.processor_thread_id_tx = Some(processor_tx);
    }
    
    fn register_current_thread_as_fetcher(&self) {
        if let Some(tx) = &self.fetcher_thread_id_tx {
            let thread_id = std::thread::current().id();
            match self.fetcher_thread_id.lock() {
                Ok(mut guard) => {
                    *guard = Some(thread_id.clone());
                    let _ = tx.try_send(("fetcher".to_string(), thread_id));
                },
                Err(e) => {
                    error!("Failed to lock fetcher_thread_id: {}", e);
                }
            }
        }
    }
    
    fn register_current_thread_as_processor(&self) {
        if let Some(tx) = &self.processor_thread_id_tx {
            let thread_id = std::thread::current().id();
            match self.processor_thread_id.lock() {
                Ok(mut guard) => {
                    *guard = Some(thread_id.clone());
                    let _ = tx.try_send(("processor".to_string(), thread_id));
                },
                Err(e) => {
                    error!("Failed to lock processor_thread_id: {}", e);
                }
            }
        }
    }
    
    // These methods might be useful in the future, so we'll keep them but mark them as allowed dead code
    #[allow(dead_code)]
    fn is_fetcher_thread(&self) -> bool {
        match self.fetcher_thread_id.lock() {
            Ok(guard) => {
                if let Some(fetcher_id) = *guard {
                    std::thread::current().id() == fetcher_id
                } else {
                    false
                }
            },
            Err(_) => false
        }
    }
    
    #[allow(dead_code)]
    fn is_processor_thread(&self) -> bool {
        match self.processor_thread_id.lock() {
            Ok(guard) => {
                if let Some(processor_id) = *guard {
                    std::thread::current().id() == processor_id
                } else {
                    false
                }
            },
            Err(_) => false
        }
    }
}

#[post("/")]
async fn handle_jsonrpc(
    body: web::Json<JsonRpcRequest>,
    state: web::Data<AppState>,
) -> ActixResult<impl Responder> {
    debug!("RPC request: {}", serde_json::to_string(&body).unwrap());

    if body.method == "metashrew_view" {
        if body.params.len() < 3 {
            return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [view_name, input_data, height]".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            }));
        }

        let view_name = match body.params[0].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: view_name must be a string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let input_hex = match body.params[1].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: input_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let height = match &body.params[2] {
            Value::String(s) if s == "latest" => CURRENT_HEIGHT.load(Ordering::SeqCst),
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        // Acquire a read lock on the runtime - this allows multiple readers
        let runtime = state.runtime.read().await;
        
        // Decode input data outside the lock if possible
        let input_data = match hex::decode(input_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: format!("Invalid hex input: {}", e),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        };

        // Use await with the async view function
        match runtime.view(
            view_name,
            &input_data,
            height,
        ).await {
            Ok(result) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(result)),
                jsonrpc: "2.0".to_string(),
            })),
            Err(err) => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: err.to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else if body.method == "metashrew_preview" {
        // Acquire a read lock on the runtime - this allows multiple readers
        let runtime = state.runtime.read().await;
        // Ensure we have required params
        if body.params.len() < 4 {
            return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [block_data, view_name, input_data, height]"
                        .to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            }));
        }

        let block_hex = match body.params[0].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: block_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let view_name = match body.params[1].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: view_name must be a string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let input_hex = match body.params[2].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: input_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let height = match &body.params[3] {
            Value::String(s) if s == "latest" => CURRENT_HEIGHT.load(Ordering::SeqCst),
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        // Decode input data outside the lock if possible
        let block_data = match hex::decode(block_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: format!("Invalid hex block data: {}", e),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };
        
        let input_data = match hex::decode(input_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: format!("Invalid hex input: {}", e),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        };
        
        match runtime.preview_async(
          &block_data,
          view_name,
          &input_data,
          height,
      ).await {
            Ok(result) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(result)),
                jsonrpc: "2.0".to_string(),
            })),
            Err(err) => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: err.to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else if body.method == "metashrew_height" {
        // No need to lock the runtime for this operation
        Ok(HttpResponse::Ok().json(JsonRpcResult {
            id: body.id,
            result: CURRENT_HEIGHT.load(Ordering::SeqCst).to_string(),
            jsonrpc: "2.0".to_string(),
        }))
    } else if body.method == "metashrew_getblockhash" {
        // Acquire a read lock on the runtime
        let runtime = state.runtime.read().await;
        if body.params.len() != 1 {
            return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [block_number]".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            }));
        }

        let height = match &body.params[0] {
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: block_number must be a number".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let key = (String::from(HEIGHT_TO_HASH) + &height.to_string()).into_bytes();
        
        // Fix lifetime issue by storing the context in a variable
        let mut context = runtime.context.lock().unwrap();
        let result = context.db.get(&key).map_err(|_| {
            <anyhow::Error as Into<IndexerError>>::into(anyhow!(
                "DB connection error while fetching blockhash"
            ))
        })?;
        
        match result {
            Some(hash) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(hash)),
                jsonrpc: "2.0".to_string(),
            })),
            None => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: "Block hash not found".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else {
        Ok(HttpResponse::Ok().json(JsonRpcError {
            id: body.id,
            error: JsonRpcErrorObject {
                code: -32601,
                message: format!("Method '{}' not found", body.method),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        }))
    }
}

fn main() -> Result<()> {
    // Initialize the logger
    env_logger::init();
    
    // Parse command line arguments
    let args = Arc::new(Args::parse());

    if let Some(ref label) = args.label {
        set_label(label.clone());
    }

    let start_block = args.start_block.unwrap_or(0);

    // Create a custom runtime with dedicated threads for critical tasks
    // Dynamically determine the number of worker threads based on available CPUs
    let available_cpus = num_cpus::get();
    let worker_threads = std::cmp::max(8, available_cpus); // Use at least 8 threads, or more if available
    
    info!("Detected {} CPU cores, configuring tokio runtime with {} worker threads", available_cpus, worker_threads);
    
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .thread_name("metashrew-worker")
        .on_thread_start(|| {
            let thread_name = std::thread::current().name().unwrap_or("unknown").to_string();
            info!("Thread started: {}", thread_name);
        })
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    
    // Run the main application logic on the runtime
    runtime.block_on(async_main(args, start_block))
}

// The actual async main function that will run on our custom runtime
async fn async_main(args: Arc<Args>, start_block: u32) -> Result<()> {
    info!("Starting Metashrew with dedicated threads for indexer tasks");
    // No longer need thread names as they're set directly in the registration functions
    
    // Configure thread priorities using thread names
    info!("Setting up dedicated task threads");
    
    // Parse the daemon RPC URL to check for SSH tunneling
    info!("Parsing daemon RPC URL: {}", args.daemon_rpc_url);
    let (rpc_url, bypass_ssl, tunnel_config) = parse_daemon_rpc_url(&args.daemon_rpc_url)?;
    
    if let Some(config) = &tunnel_config {
        info!("SSH tunneling enabled: {}@{}:{} -> {}:{}",
            if config.ssh_user.is_empty() { "<from config>" } else { &config.ssh_user },
            config.ssh_host, config.ssh_port,
            config.target_host, config.target_port);
    }
    
    if bypass_ssl {
        info!("SSL certificate validation will be bypassed for localhost connections");
    }
    
    // Configure RocksDB options for optimal performance
    let mut opts = Options::default();
    
    // Dynamically configure RocksDB based on available CPU cores
    let available_cpus = num_cpus::get();
    
    // Calculate optimal background jobs - use approximately 1/4 of available cores
    // with a minimum of 4 and a reasonable maximum to avoid excessive context switching
    let background_jobs: i32 = std::cmp::min(
        std::cmp::max(4, available_cpus / 4),
        16  // Cap at a reasonable maximum
    ).try_into().unwrap();
    
    // Calculate write buffer number based on available cores
    let write_buffer_number: i32 = std::cmp::min(
        std::cmp::max(6, available_cpus / 6),
        12  // Cap at a reasonable maximum
    ).try_into().unwrap();
    
    info!("Configuring RocksDB with {} background jobs and {} write buffers", background_jobs, write_buffer_number);
    
    opts.create_if_missing(true);
    opts.set_max_open_files(10000);
    opts.set_use_fsync(false);
    opts.set_bytes_per_sync(8388608); // 8MB
    opts.optimize_for_point_lookup(1024);
    opts.set_table_cache_num_shard_bits(6);
    opts.set_max_write_buffer_number(write_buffer_number);
    opts.set_write_buffer_size(256 * 1024 * 1024);
    opts.set_target_file_size_base(256 * 1024 * 1024);
    opts.set_min_write_buffer_number_to_merge(2);
    opts.set_level_zero_file_num_compaction_trigger(4);
    opts.set_level_zero_slowdown_writes_trigger(20);
    opts.set_level_zero_stop_writes_trigger(30);
    opts.set_max_background_jobs(background_jobs);
    // Removed deprecated call to set_max_background_compactions
    opts.set_disable_auto_compactions(false);

    // Create runtime with RocksDB adapter
    let runtime = Arc::new(RwLock::new(MetashrewRuntime::load(
      PathBuf::from(&args.indexer),
      RocksDBRuntimeAdapter::open(args.db_path.clone(), opts)?,
  )?));

    // Create indexer state
    let mut indexer = IndexerState {
        runtime: runtime.clone(),
        args: args.clone(),
        start_block,
        fetcher_thread_id_tx: None,
        processor_thread_id_tx: None,
        fetcher_thread_id: std::sync::Mutex::new(None),
        processor_thread_id: std::sync::Mutex::new(None),
        // SSH tunnel configuration
        rpc_url,
        bypass_ssl,
        tunnel_config,
        // Initialize new fields
        active_tunnel: std::sync::Mutex::new(None),
        cached_auth: std::sync::Mutex::new(None),
        last_auth_error: std::sync::Mutex::new(None),
    };
    
    // Log the pipeline size configuration
    match args.pipeline_size {
        Some(size) => info!("Using user-specified pipeline size: {}", size),
        None => {
            let available_cpus = num_cpus::get();
            let auto_size = std::cmp::min(
                std::cmp::max(5, available_cpus / 2),
                16
            );
            info!("Using auto-configured pipeline size: {} (based on {} CPU cores)", auto_size, available_cpus);
        }
    }

    // Create app state for JSON-RPC server
    let app_state = web::Data::new(AppState {
        runtime: runtime.clone(),
    });

    // Create a channel to communicate thread IDs
    let (thread_id_tx, mut thread_id_rx) = tokio::sync::mpsc::channel::<(String, std::thread::ThreadId)>(2);
    
    // Spawn a task to monitor thread IDs
    tokio::spawn(async move {
        while let Some((role, thread_id)) = thread_id_rx.recv().await {
            info!("Thread role registered: {} on thread {:?}", role, thread_id);
        }
    });
    
    // Configure the indexer to use thread ID tracking
    indexer.set_thread_id_senders(thread_id_tx.clone(), thread_id_tx.clone());
    
    // Start the indexer in a separate task
    let indexer_handle = tokio::spawn(async move {
        info!("Starting indexer task");
        
        if let Err(e) = indexer.run_pipeline().await {
            error!("Indexer error: {}", e);
        }
    });

    // Start the JSON-RPC server
    let server_handle = tokio::spawn({
        let args_clone = args.clone();
        HttpServer::new(move || {
            let cors = match &args_clone.cors {
                Some(cors_value) if cors_value == "*" => {
                    // Allow all origins
                    Cors::default()
                        .allow_any_origin()
                        .allow_any_method()
                        .allow_any_header()
                }
                Some(cors_value) => {
                    // Allow specific origins
                    let mut cors_builder = Cors::default();
                    for origin in cors_value.split(',') {
                        cors_builder = cors_builder.allowed_origin(origin.trim());
                    }
                    cors_builder
                }
                None => {
                    // Default: only allow localhost
                    Cors::default()
                        .allowed_origin_fn(|origin, _| {
                            if let Ok(origin_str) = origin.to_str() {
                                origin_str.starts_with("http://localhost:")
                            } else {
                                false
                            }
                        })
                }
            };

            App::new()
                .wrap(cors)
                .app_data(app_state.clone())
                .service(handle_jsonrpc)
        })
        .bind((args.host.as_str(), args.port))?
        .run()
    });
    info!("Server running at http://{}:{}", args.host, args.port);

    // Wait for either component to finish (or fail)
    tokio::select! {
        result = indexer_handle => {
            if let Err(e) = result {
                log::error!("Indexer task failed: {}", e);
            }
        }
        result = server_handle => {
            if let Err(e) = result {
                log::error!("Server task failed: {}", e);
            }
        }
    }

    Ok(())
}
