use anyhow::{anyhow, Result};
use log::{debug, info};
use reqwest::{Client, ClientBuilder, Response};
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
        let mut process = cmd.stdout(Stdio::null())
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
            parsed_url.set_host(Some("localhost")).map_err(|_| anyhow!("Failed to set host"))?;
            parsed_url.set_port(Some(tunnel.local_port)).map_err(|_| anyhow!("Failed to set port"))?;
            
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
