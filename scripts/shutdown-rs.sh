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
    echo "shutdown-rs.sh # Found .env file at $ENV_FILE"
    set -o allexport
    source "$ENV_FILE"
    set +o allexport
else
    echo "shutdown-rs.sh # Error: .env file not found at $ENV_FILE"
    exit 1
fi

# Valid app names
VALID_APPS=("vega-rs" "oops-rs" "whistleblower-rs")

# Function to validate process name
validate_process() {
    local pid=$1
    # Below is required to prevent `ps` from truncating the process name
    local proc_name=$(ps -p "$pid" -o args= 2>/dev/null | awk '{print $1}' | xargs basename 2>/dev/null)
    
    for app in "${VALID_APPS[@]}"; do
        if [ "$proc_name" = "$app" ]; then
            echo "shutdown-rs.sh # About to shutdown $proc_name"
            return 0
        fi
    done
    return 1
}

# Function to stop an application using its PID file
stop_app() {
    local pid_file="$1"
    if [ -f "$pid_file" ]; then
        local pid=$(cat "$pid_file")
        if validate_process "$pid"; then
            if kill -0 "$pid" 2>/dev/null; then
                echo "shutdown-rs.sh # Stopping process with PID: $pid"
                kill "$pid"
                # Wait for process to terminate
                for i in {1..3}; do
                    if ! kill -0 "$pid" 2>/dev/null; then
                        echo "shutdown-rs.sh # Process $pid terminated"
                        rm "$pid_file"
                        return 0
                    fi
                    sleep 1
                done
                # Force kill if still running
                echo "shutdown-rs.sh # Process $pid not responding, force killing..."
                kill -9 "$pid" 2>/dev/null
            else
                echo "shutdown-rs.sh # Process $pid not running"
            fi
        else
            echo "shutdown-rs.sh # Warning: Process $pid does not match expected app names"
        fi
        rm "$pid_file"
    fi
}

# Stop all applications by reading PID files
for pid_file in "$PID_DIR"/*; do
    if [ -f "$pid_file" ]; then
        stop_app "$pid_file"
    fi
done

# Clean up PID directory if empty
if [ -z "$(ls -A $PID_DIR)" ]; then
    rm -rf "$PID_DIR"
    echo "shutdown-rs.sh # Removed empty PID directory"
fi

echo "shutdown-rs.sh # Shutdown complete"
