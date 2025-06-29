#!/bin/bash

# Debug Setup Script for rockshrew-mono
# This script helps set up various debugging tools for performance analysis

set -e

echo "🔧 Rockshrew-mono Debug Setup Script"
echo "======================================"

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to install tokio-console
install_tokio_console() {
    echo "📦 Installing tokio-console..."
    if command_exists cargo; then
        cargo install tokio-console
        echo "✅ tokio-console installed successfully"
    else
        echo "❌ Cargo not found. Please install Rust first."
        exit 1
    fi
}

# Function to install flamegraph
install_flamegraph() {
    echo "📦 Installing flamegraph..."
    if command_exists cargo; then
        cargo install flamegraph
        echo "✅ flamegraph installed successfully"
    else
        echo "❌ Cargo not found. Please install Rust first."
        exit 1
    fi
}

# Function to install system tools
install_system_tools() {
    echo "📦 Installing system debugging tools..."
    
    if command_exists apt-get; then
        # Ubuntu/Debian
        sudo apt-get update
        sudo apt-get install -y gdb strace perf-tools-unstable linux-tools-generic htop iotop
    elif command_exists yum; then
        # CentOS/RHEL
        sudo yum install -y gdb strace perf htop iotop
    elif command_exists dnf; then
        # Fedora
        sudo dnf install -y gdb strace perf htop iotop
    elif command_exists pacman; then
        # Arch Linux
        sudo pacman -S gdb strace perf htop iotop
    else
        echo "⚠️  Unknown package manager. Please install gdb, strace, perf, htop, and iotop manually."
    fi
}

# Function to build rockshrew-mono with debugging features
build_debug_version() {
    echo "🔨 Building rockshrew-mono with debugging features..."
    
    # Check if we're in the right directory
    if [ ! -f "Cargo.toml" ] || [ ! -d "crates/rockshrew-mono" ]; then
        echo "❌ Please run this script from the metashrew project root directory"
        exit 1
    fi
    
    # Build with console feature and tokio unstable
    echo "Building with console support..."
    RUSTFLAGS="--cfg tokio_unstable" cargo build --release -p rockshrew-mono --features console
    
    echo "Building debug version..."
    RUSTFLAGS="--cfg tokio_unstable" cargo build -p rockshrew-mono --features console,debug-tracing
    
    echo "✅ Debug builds completed"
}

# Function to create debugging scripts
create_debug_scripts() {
    echo "📝 Creating debugging scripts..."
    
    # Create tokio-console debugging script
    cat > debug-with-console.sh << 'EOF'
#!/bin/bash
# Run rockshrew-mono with tokio-console support

echo "🚀 Starting rockshrew-mono with tokio-console support"
echo "In another terminal, run: tokio-console"
echo "Press Ctrl+C to stop"

export ROCKSHREW_CONSOLE=1
export RUST_LOG="tokio=trace,runtime=trace,rockshrew_mono=debug,info"

# Use debug build for better debugging experience
RUSTFLAGS="--cfg tokio_unstable" ./target/debug/rockshrew-mono "$@"
EOF
    chmod +x debug-with-console.sh
    
    # Create performance profiling script
    cat > profile-performance.sh << 'EOF'
#!/bin/bash
# Profile rockshrew-mono performance

if [ $# -eq 0 ]; then
    echo "Usage: $0 <PID> [duration_seconds]"
    echo "Find PID with: ps aux | grep rockshrew-mono"
    exit 1
fi

PID=$1
DURATION=${2:-30}

echo "🔍 Profiling rockshrew-mono (PID: $PID) for $DURATION seconds..."

# Check if process exists
if ! kill -0 $PID 2>/dev/null; then
    echo "❌ Process $PID not found"
    exit 1
fi

# Profile with perf
echo "📊 Running perf record..."
sudo perf record -p $PID -g --call-graph dwarf sleep $DURATION

# Generate flamegraph
echo "🔥 Generating flamegraph..."
if command -v flamegraph >/dev/null 2>&1; then
    sudo perf script | stackcollapse-perf.pl | flamegraph.pl > rockshrew-profile.svg
    echo "✅ Flamegraph saved to: rockshrew-profile.svg"
else
    echo "⚠️  flamegraph not installed. Install with: cargo install flamegraph"
    echo "📊 Perf report available with: sudo perf report"
fi
EOF
    chmod +x profile-performance.sh
    
    # Create system monitoring script
    cat > monitor-system.sh << 'EOF'
#!/bin/bash
# Monitor system resources for rockshrew-mono

if [ $# -eq 0 ]; then
    echo "Usage: $0 <PID>"
    echo "Find PID with: ps aux | grep rockshrew-mono"
    exit 1
fi

PID=$1

echo "📊 Monitoring system resources for rockshrew-mono (PID: $PID)"
echo "Press Ctrl+C to stop"

# Check if process exists
if ! kill -0 $PID 2>/dev/null; then
    echo "❌ Process $PID not found"
    exit 1
fi

echo "=== Process Info ==="
ps -p $PID -o pid,ppid,cmd,pcpu,pmem,vsz,rss,etime

echo "=== Memory Usage ==="
cat /proc/$PID/status | grep -E "(VmSize|VmRSS|VmData|VmStk|VmExe|VmLib)"

echo "=== Open Files ==="
lsof -p $PID | wc -l
echo "files open"

echo "=== Network Connections ==="
netstat -tulpn | grep $PID

echo "=== I/O Statistics ==="
cat /proc/$PID/io

echo ""
echo "🔄 Starting continuous monitoring (updates every 5 seconds)..."
echo "CPU% MEM% VSZ RSS COMMAND"

while true; do
    if ! kill -0 $PID 2>/dev/null; then
        echo "❌ Process $PID terminated"
        break
    fi
    
    ps -p $PID -o pcpu,pmem,vsz,rss,cmd --no-headers
    sleep 5
done
EOF
    chmod +x monitor-system.sh
    
    # Create strace monitoring script
    cat > trace-syscalls.sh << 'EOF'
#!/bin/bash
# Trace system calls for rockshrew-mono

if [ $# -eq 0 ]; then
    echo "Usage: $0 <PID> [trace_type]"
    echo "Find PID with: ps aux | grep rockshrew-mono"
    echo "trace_type options:"
    echo "  all     - trace all system calls (default)"
    echo "  file    - trace file operations"
    echo "  network - trace network operations"
    echo "  count   - count system calls"
    exit 1
fi

PID=$1
TRACE_TYPE=${2:-all}

echo "🔍 Tracing system calls for rockshrew-mono (PID: $PID)"
echo "Press Ctrl+C to stop"

# Check if process exists
if ! kill -0 $PID 2>/dev/null; then
    echo "❌ Process $PID not found"
    exit 1
fi

case $TRACE_TYPE in
    "file")
        echo "📁 Tracing file operations..."
        sudo strace -p $PID -e trace=openat,read,write,close,fsync,fdatasync -T -tt
        ;;
    "network")
        echo "🌐 Tracing network operations..."
        sudo strace -p $PID -e trace=socket,connect,accept,send,recv,sendto,recvfrom -T -tt
        ;;
    "count")
        echo "📊 Counting system calls..."
        sudo strace -p $PID -c -S time
        ;;
    *)
        echo "🔍 Tracing all system calls..."
        sudo strace -p $PID -T -tt
        ;;
esac
EOF
    chmod +x trace-syscalls.sh
    
    echo "✅ Debug scripts created:"
    echo "  - debug-with-console.sh: Run with tokio-console support"
    echo "  - profile-performance.sh: Profile CPU performance"
    echo "  - monitor-system.sh: Monitor system resources"
    echo "  - trace-syscalls.sh: Trace system calls"
}

# Function to show usage instructions
show_usage() {
    echo ""
    echo "🎯 Usage Instructions"
    echo "===================="
    echo ""
    echo "1. 📊 Real-time async debugging with tokio-console:"
    echo "   ./debug-with-console.sh [your-rockshrew-mono-args]"
    echo "   # In another terminal:"
    echo "   tokio-console"
    echo ""
    echo "2. 🔥 CPU performance profiling:"
    echo "   # Start your rockshrew-mono normally, then:"
    echo "   ./profile-performance.sh <PID> [duration]"
    echo ""
    echo "3. 📈 System resource monitoring:"
    echo "   ./monitor-system.sh <PID>"
    echo ""
    echo "4. 🔍 System call tracing:"
    echo "   ./trace-syscalls.sh <PID> [all|file|network|count]"
    echo ""
    echo "5. 🐛 Enhanced logging (without console):"
    echo "   ROCKSHREW_DEBUG=1 RUST_LOG=debug ./target/debug/rockshrew-mono [args]"
    echo ""
    echo "6. 📝 JSON structured logging:"
    echo "   ROCKSHREW_JSON_TRACING=1 RUST_LOG=debug ./target/debug/rockshrew-mono [args]"
    echo ""
    echo "Find process ID with: ps aux | grep rockshrew-mono"
}

# Main script logic
main() {
    echo "What would you like to set up?"
    echo "1) Install all debugging tools"
    echo "2) Install tokio-console only"
    echo "3) Install flamegraph only"
    echo "4) Install system tools only"
    echo "5) Build debug version of rockshrew-mono"
    echo "6) Create debugging scripts"
    echo "7) Show usage instructions"
    echo "8) Do everything (recommended)"
    
    read -p "Enter your choice (1-8): " choice
    
    case $choice in
        1)
            install_tokio_console
            install_flamegraph
            install_system_tools
            ;;
        2)
            install_tokio_console
            ;;
        3)
            install_flamegraph
            ;;
        4)
            install_system_tools
            ;;
        5)
            build_debug_version
            ;;
        6)
            create_debug_scripts
            ;;
        7)
            show_usage
            ;;
        8)
            install_tokio_console
            install_flamegraph
            install_system_tools
            build_debug_version
            create_debug_scripts
            show_usage
            ;;
        *)
            echo "❌ Invalid choice"
            exit 1
            ;;
    esac
}

# Run main function
main "$@"

echo ""
echo "✅ Debug setup complete!"
echo "📖 See debug-rockshrew-mono.md for detailed usage instructions"