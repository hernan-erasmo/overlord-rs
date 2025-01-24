#!/bin/bash

# Directory to store PID files
PID_DIR=".tmp/overlord-apps-pids"

# Check required environment variables ie.
# export VEGA_RS_BIN_PATH=~/projects/overlord-rs/target/release/vega-rs
# export OOPS_RS_BIN_PATH=~/projects/overlord-rs/target/release/oops-rs
# export WHISTLEBLOWER_RS_BIN_PATH=~/projects/overlord-rs/target/release/whistleblower-rs

required_vars=("VEGA_RS_BIN_PATH" "WHISTLEBLOWER_RS_BIN_PATH" "OOPS_RS_BIN_PATH")
missing_vars=()

for var in "${required_vars[@]}"; do
    if [ -z "${!var}" ]; then
        missing_vars+=("$var")
    fi
done

if [ ${#missing_vars[@]} -ne 0 ]; then
    echo "Error: The following required environment variables are not set:"
    printf '%s\n' "${missing_vars[@]}"
    exit 1
fi

# Remove existing PID directory and create a new one
rm -rf "$PID_DIR"
mkdir -p "$PID_DIR"

# Function to start an application and store its PID
start_app() {
    local app_path="$1"
    local pid_file="$2"
    
    # Start the application with setsid and store PID
    setsid "$app_path" > /dev/null 2>&1 &
    echo $! > "$pid_file"
    echo "Started $app_path with PID $(cat $pid_file)"
}

# Start applications and store their PIDs
start_app "$VEGA_RS_BIN_PATH" "$PID_DIR/vega-rs.pid"
start_app "$WHISTLEBLOWER_RS_BIN_PATH" "$PID_DIR/whistleblower-rs.pid"
start_app "$OOPS_RS_BIN_PATH" "$PID_DIR/oops-rs.pid"

echo "All applications started successfully"
