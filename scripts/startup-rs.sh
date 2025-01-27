#!/bin/bash

########################################################################
# overlord-rs startup script
#
# 99% of the time you'll want to use this to run overlord
# (unless you're debugging something or know what you're doing)
#
# Available args:
# --ignore-temp: Clear the temporary input directory without prompting
#
########################################################################

# Get script directory regardless of how it's called
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ENV_FILE="$SCRIPT_DIR/../.env"

# Load environment variables from .env file in parent directory
if [ -f "$ENV_FILE" ]; then
    echo "startup-rs.sh # Found .env file at $ENV_FILE"
    set -o allexport
    source "$ENV_FILE"
    set +o allexport
else
    echo "startup-rs.sh # Error: .env file not found at $ENV_FILE"
    exit 1
fi

# Check if --ignore-temp argument was provided
if [[ " $* " =~ " --ignore-temp " ]]; then
    echo "startup-rs.sh # Ignoring contents of TEMP_INPUT_DIR at $TEMP_INPUT_DIR and recreating it."
    rm -rf "$TEMP_INPUT_DIR"
    mkdir -p "$TEMP_INPUT_DIR"
else
    if [[ -d "$TEMP_INPUT_DIR" ]] && [[ "$(ls -A "$TEMP_INPUT_DIR" 2>/dev/null)" ]]; then
        echo "startup-rs.sh # Warning: $TEMP_INPUT_DIR contains files that may be important."
        echo "startup-rs.sh # Use --ignore-temp to clear the directory and continue."
        exit 1
    fi
    echo "startup-rs.sh # TEMP_INPUT_DIR directory already exists at $TEMP_INPUT_DIR, but is empty."
fi

# Remove existing PID directory and create a new one
echo "startup-rs.sh # Clearing $PID_DIR directory and creating a new one"
rm -rf "$PID_DIR"
mkdir -p "$PID_DIR"

start_vega() {
    local use_local_data=true

    # Refactor this if options other than --use-local-data are required
    if [ $# -gt 0 ]; then
        use_local_data=false
    fi

    echo "startup-rs.sh # Running $SCRIPT_DIR/run_pmex.py with use-local-data=$use_local_data"
    VEGA_ADDRESSES_FILE=$(DATA_DIR="$DATA_DIR" python3 $SCRIPT_DIR/run_pmex.py --use-local-data="$use_local_data" | tee /dev/tty | tail -n1)

    # Check if output is empty
    if [ -z "$VEGA_ADDRESSES_FILE" ]; then
        echo "startup-rs.sh # run_pmex.py returned empty output. Aborting startup." >&2
        exit 1
    fi

    echo "startup-rs.sh # Attempting to start vega-rs with"
    echo "startup-rs.sh #    - VEGA_ADDRESSES_FILE=$VEGA_ADDRESSES_FILE"
    echo "startup-rs.sh #    - VEGA_CHAINLINK_ADDRESSES_FILE=$VEGA_CHAINLINK_ADDRESSES_FILE"
    setsid env \
        VEGA_ADDRESSES_FILE="$VEGA_ADDRESSES_FILE" \
        VEGA_CHAINLINK_ADDRESSES_FILE="$VEGA_CHAINLINK_ADDRESSES_FILE" \
        "$VEGA_RS_BIN_PATH" > /dev/null 2>&1 &
    echo $! > "$PID_DIR/vega-rs.pid"
    echo "startup-rs.sh # Started $VEGA_RS_BIN_PATH with PID $(cat $PID_DIR/vega-rs.pid)"
}

# Function to start an application and store its PID
start_app() {
    local app_path="$1"
    local pid_file="$2"
    
    # Start the application with setsid and store PID
    setsid "$app_path" > /dev/null 2>&1 &
    echo $! > "$pid_file"
    echo "startup-rs.sh # Started $app_path with PID $(cat $pid_file)"
}

# Start applications and store their PIDs
start_vega
#start_app "$WHISTLEBLOWER_RS_BIN_PATH" "$PID_DIR/whistleblower-rs.pid"
#start_app "$OOPS_RS_BIN_PATH" "$PID_DIR/oops-rs.pid"

echo "startup-rs.sh # All applications started successfully"
