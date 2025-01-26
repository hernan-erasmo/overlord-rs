#!/bin/bash

# Directory containing PID files
PID_DIR=".tmp/overlord-apps-pids"

# Valid app names
VALID_APPS=("vega-rs" "oops-rs" "whistleblower-rs")

# Function to validate process name
validate_process() {
    local pid=$1
    # Below is required to prevent `ps` from truncating the process name
    local proc_name=$(ps -p "$pid" -o args= 2>/dev/null | awk '{print $1}' | xargs basename 2>/dev/null)
    
    for app in "${VALID_APPS[@]}"; do
        if [ "$proc_name" = "$app" ]; then
            echo "About to shutdown $proc_name"
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
        if kill -0 "$pid" 2>/dev/null; then
            if validate_process "$pid"; then
                echo "Stopping process with PID: $pid"
                kill "$pid"
                # Wait for process to terminate
                for i in {1..3}; do
                    if ! kill -0 "$pid" 2>/dev/null; then
                        echo "Process $pid terminated"
                        rm "$pid_file"
                        return 0
                    fi
                    sleep 1
                done
                # Force kill if still running
                echo "Process $pid not responding, force killing..."
                kill -9 "$pid" 2>/dev/null
            else
                echo "Warning: Process $pid does not match expected app names"
            fi
        else
            echo "Process $pid not running"
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
    echo "Removed empty PID directory"
fi

echo "Shutdown complete"
