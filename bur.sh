#!/bin/bash

# Exit on any error
set -e

# Function to cleanup anvil process
cleanup() {
    if [ ! -z "$ANVIL_PID" ]; then
        echo "Cleaning up anvil process (PID: $ANVIL_PID)"
        kill $ANVIL_PID 2>/dev/null || true
    fi
}

# Set up trap for cleanup on script exit
trap cleanup EXIT

# Validate required argument
if [ -z "$1" ]; then
    echo "Error: User address is required"
    echo "Usage: $0 <user_address> [block_number]"
    exit 1
fi

USER_ADDRESS=$1
ANVIL_IPC="/tmp/bur_anvil_instance.ipc"

# Start anvil with appropriate parameters
if [ -n "$2" ]; then
    echo "Starting anvil with block number $2..."
    anvil --ipc $ANVIL_IPC --fork-url /tmp/reth.ipc --fork-block-number "$2" > /dev/null 2>&1 &
else
    echo "Starting anvil at latest block..."
    anvil --ipc $ANVIL_IPC --fork-url /tmp/reth.ipc > /dev/null 2>&1 &
fi

ANVIL_PID=$!

# Wait for anvil to start and IPC file to be created
echo "Waiting for anvil to start..."
for i in {1..30}; do
    if [ -S $ANVIL_IPC ]; then
        break
    fi
    sleep 1
    if [ $i -eq 30 ]; then
        echo "Error: Anvil failed to start (timeout waiting for IPC file)"
        exit 1
    fi
done

echo "Building project..."
if ! cargo build --release --workspace > /dev/null 2>&1; then
    echo "Error: Build failed"
    exit 1
fi

echo "Running bpchecker..."
./target/release/bpchecker "$USER_ADDRESS" "$ANVIL_IPC"

echo "Done!"
