#!/bin/bash

# Script to apply improvements to rockshrew-mono
# This script automates the process of applying the improvements to the rockshrew-mono component

set -e  # Exit on error

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Starting application of improvements to rockshrew-mono...${NC}"

# Check if we're in the right directory
if [ ! -d "rockshrew-mono" ]; then
    if [ -f "rockshrew-mono/apply_improvements.sh" ]; then
        echo -e "${YELLOW}Already in the project root directory.${NC}"
    else
        echo -e "${RED}Error: Please run this script from the project root directory (containing rockshrew-mono).${NC}"
        exit 1
    fi
fi

# Create improvements.rs if it doesn't exist
if [ ! -f "rockshrew-mono/src/improvements.rs" ]; then
    echo -e "${YELLOW}Creating improvements.rs file...${NC}"
    cp rockshrew-mono/apply_improvements_fixed.sh rockshrew-mono/src/improvements.rs
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to create improvements.rs file.${NC}"
        exit 1
    fi
    echo -e "${GREEN}Created improvements.rs file.${NC}"
else
    echo -e "${GREEN}improvements.rs file already exists.${NC}"
fi

# Update Cargo.toml to add url dependency
echo -e "${YELLOW}Updating Cargo.toml to add url dependency...${NC}"
if ! grep -q "url = " rockshrew-mono/Cargo.toml; then
    # Add url dependency
    sed -i '/num_cpus/a url = "2.5.0"  # Added for URL parsing in SSH tunneling' rockshrew-mono/Cargo.toml
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to update Cargo.toml.${NC}"
        exit 1
    fi
    echo -e "${GREEN}Updated Cargo.toml with url dependency.${NC}"
else
    echo -e "${GREEN}url dependency already exists in Cargo.toml.${NC}"
fi

# Update main.rs to use the improvements module
echo -e "${YELLOW}Updating main.rs to use the improvements module...${NC}"

# Check if improvements module is already imported
if ! grep -q "mod improvements;" rockshrew-mono/src/main.rs; then
    # Add the improvements module import
    sed -i '/use tokio::time::sleep;/a \
\
// Import the improvements module\
mod improvements;\
use improvements::{parse_daemon_rpc_url, make_request_with_tunnel, SshTunnelConfig};' rockshrew-mono/src/main.rs
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to add improvements module import to main.rs.${NC}"
        exit 1
    fi
    echo -e "${GREEN}Added improvements module import to main.rs.${NC}"
else
    echo -e "${GREEN}improvements module already imported in main.rs.${NC}"
fi

# Update IndexerState struct to add SSH tunnel configuration
if ! grep -q "tunnel_config:" rockshrew-mono/src/main.rs; then
    echo -e "${YELLOW}Updating IndexerState struct...${NC}"
    sed -i '/struct IndexerState {/,/}/ s/}/    \/\/ SSH tunnel configuration\n    rpc_url: String,\n    bypass_ssl: bool,\n    tunnel_config: Option<SshTunnelConfig>,\n}/' rockshrew-mono/src/main.rs
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to update IndexerState struct in main.rs.${NC}"
        exit 1
    fi
    echo -e "${GREEN}Updated IndexerState struct in main.rs.${NC}"
else
    echo -e "${GREEN}IndexerState struct already updated in main.rs.${NC}"
fi

# Update post_once method to use SSH tunneling
if ! grep -q "make_request_with_tunnel" rockshrew-mono/src/main.rs; then
    echo -e "${YELLOW}Updating post_once method...${NC}"
    # Use awk to replace the post_once method
    awk '
    /async fn post_once/ {
        print "    async fn post_once(&self, body: String) -> Result<Response, reqwest::Error> {"
        print "        // Use the make_request_with_tunnel function from the improvements module"
        print "        match make_request_with_tunnel("
        print "            &self.rpc_url,"
        print "            body,"
        print "            self.args.auth.clone(),"
        print "            self.tunnel_config.clone(),"
        print "            self.bypass_ssl"
        print "        ).await {"
        print "            Ok(response) => Ok(response),"
        print "            Err(e) => Err(reqwest::Error::from(std::io::Error::new("
        print "                std::io::ErrorKind::Other, "
        print "                format!(\"Request failed: {}\", e)"
        print "            ))),"
        print "        }"
        print "    }"
        
        # Skip the original implementation
        in_method = 1
        next
    }
    
    in_method && /^    }/ {
        in_method = 0
        next
    }
    
    in_method {
        next
    }
    
    { print }
    ' rockshrew-mono/src/main.rs > rockshrew-mono/src/main.rs.new
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to update post_once method in main.rs.${NC}"
        rm -f rockshrew-mono/src/main.rs.new
        exit 1
    fi
    
    mv rockshrew-mono/src/main.rs.new rockshrew-mono/src/main.rs
    echo -e "${GREEN}Updated post_once method in main.rs.${NC}"
else
    echo -e "${GREEN}post_once method already updated in main.rs.${NC}"
fi

# Update async_main to parse daemon RPC URL
if ! grep -q "parse_daemon_rpc_url" rockshrew-mono/src/main.rs; then
    echo -e "${YELLOW}Updating async_main function...${NC}"
    # Use awk to add the daemon RPC URL parsing
    awk '
    /info\("Setting up dedicated task threads"\);/ {
        print $0
        print ""
        print "    // Parse the daemon RPC URL to check for SSH tunneling"
        print "    info!(\"Parsing daemon RPC URL: {}\", args.daemon_rpc_url);"
        print "    let (rpc_url, bypass_ssl, tunnel_config) = parse_daemon_rpc_url(&args.daemon_rpc_url)?;"
        print ""
        print "    if let Some(config) = &tunnel_config {"
        print "        info!(\"SSH tunneling enabled: {}@{}:{} -> {}:{}\", "
        print "            if config.ssh_user.is_empty() { \"<from config>\" } else { &config.ssh_user },"
        print "            config.ssh_host, config.ssh_port, "
        print "            config.target_host, config.target_port);"
        print "    }"
        print ""
        print "    if bypass_ssl {"
        print "        info!(\"SSL certificate validation will be bypassed for localhost connections\");"
        print "    }"
        print ""
        next
    }
    
    { print }
    ' rockshrew-mono/src/main.rs > rockshrew-mono/src/main.rs.new
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to update async_main function in main.rs.${NC}"
        rm -f rockshrew-mono/src/main.rs.new
        exit 1
    fi
    
    mv rockshrew-mono/src/main.rs.new rockshrew-mono/src/main.rs
    echo -e "${GREEN}Updated async_main function in main.rs.${NC}"
else
    echo -e "${GREEN}async_main function already updated in main.rs.${NC}"
fi

# Update IndexerState initialization to include SSH tunnel configuration
if ! grep -q "rpc_url," rockshrew-mono/src/main.rs; then
    echo -e "${YELLOW}Updating IndexerState initialization...${NC}"
    # Use awk to update the IndexerState initialization
    awk '
    /let mut indexer = IndexerState {/ {
        in_init = 1
        print $0
    }
    
    in_init && /processor_thread_id: std::sync::Mutex::new\(None\),/ {
        print $0
        print "        // SSH tunnel configuration"
        print "        rpc_url,"
        print "        bypass_ssl,"
        print "        tunnel_config,"
        next
    }
    
    in_init && /};/ {
        in_init = 0
    }
    
    { print }
    ' rockshrew-mono/src/main.rs > rockshrew-mono/src/main.rs.new
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to update IndexerState initialization in main.rs.${NC}"
        rm -f rockshrew-mono/src/main.rs.new
        exit 1
    fi
    
    mv rockshrew-mono/src/main.rs.new rockshrew-mono/src/main.rs
    echo -e "${GREEN}Updated IndexerState initialization in main.rs.${NC}"
else
    echo -e "${GREEN}IndexerState initialization already updated in main.rs.${NC}"
fi

# Create IMPROVEMENTS.md if it doesn't exist
if [ ! -f "rockshrew-mono/IMPROVEMENTS.md" ]; then
    echo -e "${YELLOW}Creating IMPROVEMENTS.md file...${NC}"
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
EOF
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to create IMPROVEMENTS.md file.${NC}"
        exit 1
    fi
    echo -e "${GREEN}Created IMPROVEMENTS.md file.${NC}"
else
    echo -e "${GREEN}IMPROVEMENTS.md file already exists.${NC}"
fi

echo -e "${GREEN}Successfully applied all improvements to rockshrew-mono!${NC}"
echo -e "${YELLOW}Next steps:${NC}"
echo -e "1. Run 'cargo build -p rockshrew-mono' to build the updated component"
echo -e "2. Test the SSH tunneling functionality with different URL formats"
echo -e "3. Review the IMPROVEMENTS.md file for documentation on the new features"

exit 0