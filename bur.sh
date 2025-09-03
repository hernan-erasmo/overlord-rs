#!/bin/bash

#!/bin/bash

# BUR (Backtest User Reserves) - Liquidation Analysis Tool
#
# This script helps debug and analyze past liquidations by providing detailed information
# about liquidation opportunities for a given user address.
#
# Given:
# - A user address (the user that was liquidated)
# - A price update transaction hash (containing the forward() call that triggered liquidation conditions)
# - A block number (the block BEFORE the price update transaction landed)
#
# BUR will:
# 1. Fork the blockchain at the specified block to capture pre-liquidation state
# 2. Replay the price update transaction to trigger liquidation conditions
# 3. Calculate all possible liquidation combinations (reserve/collateral pairs)
# 4. Identify the most profitable liquidation combination
# 5. Calculate exact profit amounts and input parameters for the foxdie contract
# 6. Determine the optimal flashloan provider for maximum profitability
#
# This tool is essential for:
# - Backtesting liquidation strategies
# - Analyzing missed liquidation opportunities
# - Optimizing liquidation bot parameters
# - Understanding liquidation mechanics and profitability
#
# Usage: ./bur.sh <user_address> <block_number> <tx_hash> [--use-third-party-provider <url>]
#
# Example:
#
# # If you already have a local reth node running with an IPC endpoint at /tmp/reth.ipc
# ./bur.sh 0x5f978d56aedabf9d03b8368bcab47e57c50aa06b 22512129 0x0e7d9f0ec6f83fd7d53af926fc681cfeb3fa5cd4d4e6dcc952dd7c0fdf117a12
#
# # If you want to use a third-party provider (e.g. Alchemy, Infura) instead of a local node
# # ./bur.sh 0x5f978d56aedabf9d03b8368bcab47e57c50aa06b 22512129 0x0e7d9f0ec6f83fd7d53af926fc681cfeb3fa5cd4d4e6dcc952dd7c0fdf117a12 --use-third-party-provider https://eth-mainnet.g.alchemy.com/v2/KEY
#

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

# Initialize variables with default values
PROVIDER_URL="/tmp/reth.ipc"

# Check if 4th argument is the flag and 5th argument is the URL
if [ "$4" = "--use-third-party-provider" ]; then
    if [ -z "$5" ]; then
        echo "Error: --use-third-party-provider requires a URL argument"
        echo "Example: --use-third-party-provider https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"
        exit 1
    fi
    PROVIDER_URL="$5"
fi

echo "Building project..."
if ! cargo build --release --bin bpchecker > /dev/null 2>&1; then
    echo "Error: Build failed"
    exit 1
fi

# Start anvil with appropriate parameters
if [ -n "$BLOCK_NUMBER" ]; then
    echo "Starting anvil with block number $BLOCK_NUMBER..."
    ANVIL_CMD="anvil --ipc $ANVIL_IPC --fork-url $PROVIDER_URL --fork-block-number $BLOCK_NUMBER"
    echo "Executing: $ANVIL_CMD"
    $ANVIL_CMD > /dev/null 2>&1 &
else
    echo "Starting anvil at latest block..."
    ANVIL_CMD="anvil --ipc $ANVIL_IPC --fork-url $PROVIDER_URL"
    echo "Executing: $ANVIL_CMD"
    $ANVIL_CMD > /dev/null 2>&1 &
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
export PRICE_UPDATE_FROM=$PRICE_UPDATE_FROM && export PRICE_UPDATE_TX=$PRICE_UPDATE_TX && ./target/release/bpchecker "$USER_ADDRESS" "$ANVIL_IPC"

echo "Done!"
