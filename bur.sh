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
    echo "Usage: $0 <user_address> [block_number] [tx_hash]"
    exit 1
fi

USER_ADDRESS=$1
BLOCK_NUMBER=$2
TX_HASH=$3
ANVIL_IPC="/tmp/bur_anvil_instance.ipc"
PROVIDER_URL="/tmp/reth.ipc"

# Check if fourth argument exists and matches the flag
if [ "${4:-}" = "--use-third-party-provider" ]; then
    PROVIDER_URL="https://eth-mainnet.g.alchemy.com/v2/f48E9HLwDQTbfaoDutz9P07TqfugqApS"
fi

echo "Building project..."
if ! cargo build --release --bin bpchecker > /dev/null 2>&1; then
    echo "Error: Build failed"
    exit 1
fi

# Start anvil with appropriate parameters
if [ -n "$BLOCK_NUMBER" ]; then
    echo "Starting anvil with block number $BLOCK_NUMBER..."
    anvil --ipc $ANVIL_IPC --fork-url $PROVIDER_URL --fork-block-number "$BLOCK_NUMBER" > /dev/null 2>&1 &
else
    echo "Starting anvil at latest block..."
    anvil --ipc $ANVIL_IPC --fork-url $PROVIDER_URL > /dev/null 2>&1 &
fi

ANVIL_PID=$!

# Wait for anvil to start and IPC file to be created with proper permissions
echo "Waiting for anvil to start..."
for i in {1..30}; do
    if [ -S "$ANVIL_IPC" ] && [ -r "$ANVIL_IPC" ] && [ -w "$ANVIL_IPC" ]; then
        # Add a small delay to ensure anvil is fully initialized
        sleep 2
        # Test the connection
        if cast block --rpc-url "$ANVIL_IPC" latest > /dev/null 2>&1; then
            echo "Anvil is ready!"
            break
        fi
    fi
    echo "Waiting for anvil IPC socket to be ready... ($i/30)"
    sleep 1
    if [ $i -eq 30 ]; then
        echo "Error: Anvil failed to start or IPC socket is not accessible"
        echo "IPC file exists: $([ -e "$ANVIL_IPC" ] && echo "yes" || echo "no")"
        echo "IPC file permissions: $(ls -l "$ANVIL_IPC")"
        echo "Current user: $(whoami)"
        cleanup
        exit 1
    fi
done

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

# If tx_hash is provided, validate and replay the transaction
if [ -n "$TX_HASH" ]; then
    echo -e "\n# Extracting price update information from TX"
    PRICE_UPDATE_TX=$TX_HASH

    # Get the block number of the transaction
    TX_BLOCK_NUMBER=$(cast tx --rpc-url $PROVIDER_URL $PRICE_UPDATE_TX blockNumber)

    # Validate block number
    if [ -n "$BLOCK_NUMBER" ] && [ "$BLOCK_NUMBER" -ge "$TX_BLOCK_NUMBER" ]; then
        echo "Error: Forked at provided block number $BLOCK_NUMBER, but transaction landed at $TX_BLOCK_NUMBER. Anvil should be forked at the block BEFORE the transaction"
        cleanup
        exit 1
    fi

    # Extract transaction details
    PRICE_UPDATE_TO=$(cast tx --rpc-url $PROVIDER_URL $PRICE_UPDATE_TX to)
    PRICE_UPDATE_TX_INPUT=$(cast tx --rpc-url $PROVIDER_URL $PRICE_UPDATE_TX input)
    PRICE_UPDATE_FROM=$(cast tx --rpc-url $PROVIDER_URL $PRICE_UPDATE_TX from)

    # Impersonate account
    IMPERSONATE_RESULT=$(cast rpc anvil_impersonateAccount $PRICE_UPDATE_FROM --rpc-url $ANVIL_IPC)
    if [ "$IMPERSONATE_RESULT" != "null" ]; then
        echo "Error: Failed to impersonate account $PRICE_UPDATE_FROM"
        cleanup
        exit 1
    fi

    # Replay transaction
    echo "Replaying transaction..."
    TX_STATUS=$(cast send --json --gas-limit 7000000 --rpc-url $ANVIL_IPC --unlocked --from $PRICE_UPDATE_FROM $PRICE_UPDATE_TO $PRICE_UPDATE_TX_INPUT | jq '.status')
    if [ "$TX_STATUS" != "\"0x1\"" ]; then
        echo "Error: Transaction replay failed with status $TX_STATUS"
        cleanup
        exit 1
    fi
    echo "Transaction replayed successfully"
fi

echo "Running bpchecker with user_address: $USER_ADDRESS, ipc_file: $ANVIL_IPC"
./target/release/bpchecker "$USER_ADDRESS" "$ANVIL_IPC"

echo "Done!"
