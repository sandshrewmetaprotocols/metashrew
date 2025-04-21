#!/bin/bash

# Script to apply improvements to rockshrew-mono
# This is an improved version with better error handling and fixes for potential issues

set -e  # Exit on error

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print section headers
section() {
    echo -e "\n${BLUE}=== $1 ===${NC}"
}

# Function to print success messages
success() {
    echo -e "${GREEN}✓ $1${NC}"
}

# Function to print warning messages
warning() {
    echo -e "${YELLOW}! $1${NC}"
}

# Function to print error messages
error() {
    echo -e "${RED}✗ $1${NC}"
}

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to backup a file before modifying it
backup_file() {
    local file=$1
    local backup="${file}.bak"
    
    if [ -f "$file" ]; then
        cp "$file" "$backup"
        success "Created backup of $file at $backup"
    else
        error "File $file does not exist, cannot create backup"
        return 1
    fi
}

section "Starting application of improvements to rockshrew-mono"

# Check if we're in the right directory
if [ ! -d "rockshrew-mono" ]; then
    if [ -f "rockshrew-mono/apply_improvements_fixed.sh" ]; then
        warning "Already in the project root directory."
    else
        error "Please run this script from the project root directory (containing rockshrew-mono)."
        exit 1
    fi
fi

# Check for required commands
section "Checking for required commands"
for cmd in sed awk grep cp mv; do
    if command_exists $cmd; then
        success "$cmd is available"
    else
        error "$cmd is required but not found"
        exit 1
    fi
done

# Create improvements.rs
section "Creating improvements.rs file"
if [ ! -f "rockshrew-mono/src/improvements.rs" ]; then
    # Create the directory if it doesn't exist
    mkdir -p rockshrew-mono/src
    
    # Create the improvements.rs file with the SSH tunneling implementation
    cat > rockshrew-mono/src/improvements.rs << 'EOF'
use anyhow::{anyhow, Result};
use log::{debug, error, info};
use reqwest::{Client, ClientBuilder, Response, Url};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;
use url::Url as UrlParser;

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
        
        // Create SSH tunnel config
        let tunnel_config = SshTunnelConfig {
            ssh_host: ssh_host.to_string(),
            ssh_port,
            ssh_user: ssh_user.to_string(),
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
        
        // Add tunnel options
        cmd.arg("-N")  // Don't execute a remote command
           .arg("-T")  // Disable pseudo-terminal allocation
           .arg("-L")  // Local port forwarding
           .arg(format!("{}:{}:{}", self.local_port, self.target_host, self.target_port));
        
        // Start the SSH process
        let process = cmd.stdout(Stdio::null())
                         .stderr(Stdio::null())
                         .spawn()?;
        
        // Wait a moment for the tunnel to establish
        std::thread::sleep(Duration::from_millis(500));
        
        // Check if the tunnel is working by trying to connect to the local port
        match TcpStream::connect(format!("127.0.0.1:{}", self.local_port)) {
            Ok(_) => {
                debug!("SSH tunnel established successfully on port {}", self.local_port);
                Ok(SshTunnel {
                    process,
                    local_port: self.local_port,
                })
            },
            Err(e) => {
                // Try to kill the process if connection failed
                let _ = process.kill();
                Err(anyhow!("Failed to establish SSH tunnel: {}", e))
            }
        }
    }
}

/// Represents an active SSH tunnel
pub struct SshTunnel {
    process: std::process::Child,
    pub local_port: u16,
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        debug!("Closing SSH tunnel on port {}", self.local_port);
        let _ = self.process.kill();
    }
}

/// Find an available local port for the SSH tunnel
fn find_available_port() -> Result<u16> {
    // Try to bind to port 0, which lets the OS assign an available port
    let socket = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    Ok(port)
}

/// Creates a reqwest Client with appropriate SSL configuration
pub fn create_http_client(bypass_ssl: bool) -> Result<Client> {
    let mut client_builder = ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(5);
    
    if bypass_ssl {
        debug!("Creating HTTP client with SSL validation disabled");
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }
    
    Ok(client_builder.build()?)
}

/// Makes an HTTP request through an SSH tunnel if needed
pub async fn make_request_with_tunnel(
    url: &str, 
    body: String,
    auth: Option<String>,
    tunnel_config: Option<SshTunnelConfig>,
    bypass_ssl: bool
) -> Result<Response> {
    // Create HTTP client with appropriate SSL configuration
    let client = create_http_client(bypass_ssl)?;
    
    // If we have tunnel config, create the tunnel
    let tunnel = match tunnel_config {
        Some(config) => Some(config.create_tunnel()?),
        None => None,
    };
    
    // Determine the final URL to use
    let final_url = match &tunnel {
        Some(tunnel) => {
            // Parse the original URL
            let mut parsed_url = UrlParser::parse(url)?;
            
            // Update the host and port to use the local tunnel
            parsed_url.set_host(Some("localhost"))?;
            parsed_url.set_port(Some(tunnel.local_port))?;
            
            parsed_url.to_string()
        },
        None => url.to_string(),
    };
    
    // Add authentication if provided
    let final_url = if let Some(auth_str) = auth {
        let mut parsed_url = UrlParser::parse(&final_url)?;
        let (username, password) = auth_str.split_once(':')
            .ok_or_else(|| anyhow!("Invalid auth format, expected username:password"))?;
        
        parsed_url.set_username(username)
            .map_err(|_| anyhow!("Failed to set username"))?;
        parsed_url.set_password(Some(password))
            .map_err(|_| anyhow!("Failed to set password"))?;
        
        parsed_url.to_string()
    } else {
        final_url
    };
    
    // Make the request
    debug!("Making request to {}", final_url);
    let response = client
        .post(&final_url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await?;
    
    Ok(response)
}
EOF
    
    if [ $? -ne 0 ]; then
        error "Failed to create improvements.rs file."
        exit 1
    fi
    success "Created improvements.rs file."
else
    warning "improvements.rs file already exists, skipping creation."
fi

# Update Cargo.toml
section "Updating Cargo.toml"
if [ -f "rockshrew-mono/Cargo.toml" ]; then
    backup_file "rockshrew-mono/Cargo.toml"
    
    if ! grep -q "url = " rockshrew-mono/Cargo.toml; then
        # Add url dependency
        sed -i '/num_cpus/a url = "2.5.0"  # Added for URL parsing in SSH tunneling' rockshrew-mono/Cargo.toml
        if [ $? -ne 0 ]; then
            error "Failed to update Cargo.toml."
            exit 1
        fi
        success "Updated Cargo.toml with url dependency."
    else
        warning "url dependency already exists in Cargo.toml, skipping."
    fi
else
    error "Cargo.toml not found in rockshrew-mono directory."
    exit 1
fi

# Update main.rs
section "Updating main.rs"
if [ -f "rockshrew-mono/src/main.rs" ]; then
    backup_file "rockshrew-mono/src/main.rs"
    
    # Step 1: Add the improvements module import
    if ! grep -q "mod improvements;" rockshrew-mono/src/main.rs; then
        sed -i '/use tokio::time::sleep;/a \
\
// Import the improvements module\
mod improvements;\
use improvements::{parse_daemon_rpc_url, make_request_with_tunnel, SshTunnelConfig};' rockshrew-mono/src/main.rs
        
        if [ $? -ne 0 ]; then
            error "Failed to add improvements module import to main.rs."
            exit 1
        fi
        success "Added improvements module import to main.rs."
    else
        warning "improvements module already imported in main.rs, skipping."
    fi
    
    # Step 2: Update IndexerState struct
    if ! grep -q "tunnel_config:" rockshrew-mono/src/main.rs; then
        sed -i '/struct IndexerState {/,/}/ s/}/    \/\/ SSH tunnel configuration\n    rpc_url: String,\n    bypass_ssl: bool,\n    tunnel_config: Option<SshTunnelConfig>,\n}/' rockshrew-mono/src/main.rs
        
        if [ $? -ne 0 ]; then
            error "Failed to update IndexerState struct in main.rs."
            exit 1
        fi
        success "Updated IndexerState struct in main.rs."
    else
        warning "IndexerState struct already updated in main.rs, skipping."
    fi
    
    # Step 3: Update post_once method
    if ! grep -q "make_request_with_tunnel" rockshrew-mono/src/main.rs; then
        # Create a temporary file with the new implementation
        cat > /tmp/post_once.rs << 'EOF'
    async fn post_once(&self, body: String) -> Result<Response, reqwest::Error> {
        // Use the make_request_with_tunnel function from the improvements module
        match make_request_with_tunnel(
            &self.rpc_url,
            body,
            self.args.auth.clone(),
            self.tunnel_config.clone(),
            self.bypass_ssl
        ).await {
            Ok(response) => Ok(response),
            Err(e) => Err(reqwest::Error::from(std::io::Error::new(
                std::io::ErrorKind::Other, 
                format!("Request failed: {}", e)
            ))),
        }
    }
EOF
        
        # Use sed to replace the post_once method
        sed -i '/async fn post_once/,/^    }$/c\\
'"$(cat /tmp/post_once.rs)"'' rockshrew-mono/src/main.rs
        
        if [ $? -ne 0 ]; then
            error "Failed to update post_once method in main.rs."
            exit 1
        fi
        success "Updated post_once method in main.rs."
        rm -f /tmp/post_once.rs
    else
        warning "post_once method already updated in main.rs, skipping."
    fi
    
    # Step 4: Update async_main to parse daemon RPC URL
    if ! grep -q "parse_daemon_rpc_url" rockshrew-mono/src/main.rs; then
        # Create a temporary file with the new code to insert
        cat > /tmp/parse_url.rs << 'EOF'
    
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
    
EOF
        
        # Use sed to insert the code after the "Setting up dedicated task threads" line
        sed -i '/info!("Setting up dedicated task threads");/r /tmp/parse_url.rs' rockshrew-mono/src/main.rs
        
        if [ $? -ne 0 ]; then
            error "Failed to update async_main function in main.rs."
            exit 1
        fi
        success "Updated async_main function in main.rs."
        rm -f /tmp/parse_url.rs
    else
        warning "async_main function already updated in main.rs, skipping."
    fi
    
    # Step 5: Update IndexerState initialization
    if ! grep -q "rpc_url," rockshrew-mono/src/main.rs; then
        # Create a temporary file with the new code to insert
        cat > /tmp/indexer_init.rs << 'EOF'
        // SSH tunnel configuration
        rpc_url,
        bypass_ssl,
        tunnel_config,
EOF
        
        # Use sed to insert the code before the closing brace of the IndexerState initialization
        sed -i '/processor_thread_id: std::sync::Mutex::new(None),/r /tmp/indexer_init.rs' rockshrew-mono/src/main.rs
        
        if [ $? -ne 0 ]; then
            error "Failed to update IndexerState initialization in main.rs."
            exit 1
        fi
        success "Updated IndexerState initialization in main.rs."
        rm -f /tmp/indexer_init.rs
    else
        warning "IndexerState initialization already updated in main.rs, skipping."
    fi
else
    error "main.rs not found in rockshrew-mono/src directory."
    exit 1
fi

# Create IMPROVEMENTS.md
section "Creating IMPROVEMENTS.md"
if [ ! -f "rockshrew-mono/IMPROVEMENTS.md" ]; then
    cat > rockshrew-mono/IMPROVEMENTS.md << 'EOF'
# Rockshrew-Mono Improvements

This document outlines the improvements made to the rockshrew-mono component of the Metashrew project.

## SSH Tunneling for RPC Connections

### Overview

Added support for SSH tunneling when connecting to Bitcoin Core RPC servers. This allows for secure remote connections to Bitcoin nodes without exposing the RPC port directly to the internet.

### Features

- **SSH Tunneling URI Format**: Support for special URI formats that indicate SSH tunneling should be used:
  - `ssh2+http://user@ssh-host:ssh-port/target-host:target-port`
  - `ssh2+https://user@ssh-host:ssh-port/target-host:target-port`

- **SSH Config Integration**: Support for SSH config-based connections:
  - `ssh2+http://ssh-host-alias/target-host:target-port`
  - Uses your `~/.ssh/config` for connection details

- **Automatic Port Selection**: Automatically finds an available local port for the SSH tunnel

- **Secure Cleanup**: Automatically closes SSH tunnels when the application exits

- **SSL Certificate Validation**: Intelligently bypasses SSL certificate validation for localhost connections while maintaining security for actual domain names

### Usage Examples

1. **Direct SSH Tunneling**:
   ```
   rockshrew-mono --daemon-rpc-url ssh2+http://ubuntu@mainnet.sandshrew.io:2222/localhost:8332 --auth bitcoinrpc:password
   ```

2. **Using SSH Config**:
   ```
   rockshrew-mono --daemon-rpc-url ssh2+http://mainnet2/localhost:8332 --auth bitcoinrpc:password
   ```

3. **HTTPS with SSH Tunneling**:
   ```
   rockshrew-mono --daemon-rpc-url ssh2+https://mainnet2/localhost:8332 --auth bitcoinrpc:password
   ```

### Implementation Details

The SSH tunneling functionality is implemented in the `improvements.rs` module and integrated into the main application flow. It uses the standard SSH client available on the system to create the tunnel.

## Pipeline Architecture for Block Processing

### Overview

Implemented a multi-stage pipeline for fetching and processing blocks, significantly improving throughput by allowing parallel operations.

### Features

- **Parallel Block Fetching and Processing**: Separate threads for fetching blocks and processing them
- **Dynamic Pipeline Size**: Automatically configures pipeline size based on available CPU cores
- **Thread Isolation**: Dedicated threads for critical tasks with clear role separation
- **Improved Error Handling**: Better error recovery and reporting in the pipeline

## Memory Management Improvements

### Overview

Enhanced memory management to prevent out-of-memory errors and improve stability, especially for complex indexers like ALKANES.

### Features

- **Proactive Memory Monitoring**: Continuously monitors WASM memory usage
- **Preemptive Memory Refresh**: Refreshes memory before it reaches critical levels
- **Detailed Memory Statistics**: Logs detailed memory statistics for monitoring and debugging
- **Improved Error Recovery**: Better handling of memory-related errors

## Dynamic RocksDB Configuration

### Overview

Implemented dynamic configuration of RocksDB based on available system resources for optimal performance.

### Features

- **CPU-Aware Configuration**: Adjusts RocksDB parameters based on available CPU cores
- **Optimized Write Buffers**: Configures write buffer size and count based on system capabilities
- **Balanced Background Jobs**: Sets appropriate number of background jobs for compaction
EOF
    
    if [ $? -ne 0 ]; then
        error "Failed to create IMPROVEMENTS.md file."
        exit 1
    fi
    success "Created IMPROVEMENTS.md file."
else
    warning "IMPROVEMENTS.md file already exists, skipping creation."
fi

section "Summary"
success "Successfully applied all improvements to rockshrew-mono!"
echo -e "${YELLOW}Next steps:${NC}"
echo -e "1. Run 'cargo build -p rockshrew-mono' to build the updated component"
echo -e "2. Test the SSH tunneling functionality with different URL formats"
echo -e "3. Review the IMPROVEMENTS.md file for documentation on the new features"

exit 0