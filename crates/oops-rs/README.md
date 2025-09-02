# oops-rs (Optimistic Oracle Price Scout)

The mempool monitoring component of overlord-rs that provides early warning of Chainlink price updates before they are confirmed on-chain.

## Overview

oops-rs is the "early warning system" that gives overlord-rs its competitive edge. By monitoring pending transactions in both the public mempool and MEV-Share, it can detect price updates 12-15 seconds before they're included in blocks, allowing the system to precompute liquidations and be first to execute.

## Architecture

```
┌─────────────┐    ┌─────────────┐
│   Mempool   │    │ MEV-Share   │
│  Monitor    │    │  Monitor    │
└──────┬──────┘    └──────┬──────┘
       │                  │
       └────────┬─────────┘
                │
                ▼
        ┌─────────────┐
        │Transaction  │
        │ Processor   │
        └──────┬──────┘
               │
               ▼
        ┌─────────────┐
        │   Decoder   │
        │ (Chainlink) │
        └──────┬──────┘
               │
               ▼ ZMQ
        ┌─────────────┐
        │   vega-rs   │
        └─────────────┘
```

## Key Features

### 1. Dual Source Monitoring
- **Public Mempool**: Via Reth IPC connection for standard transactions
- **MEV-Share**: Private mempool access for competitive transactions

### 2. Smart Filtering
- Tracks specific forwarder addresses that submit Chainlink updates
- Filters by `forward()` function calls containing `transmit()` data
- Maintains LRU cache to avoid reprocessing duplicate transactions

### 3. Price Extraction
The system decodes complex nested calldata:
```rust
// Extracts from: forward(address to, bytes calldata data)
// Where data contains: transmit(bytes32[3] reportContext, bytes report, ...)
```

### 4. Address Management
Dynamically resolves transmitter addresses from Chainlink aggregator contracts, supporting both:
- `getTransmitters()` - Standard interface
- `transmitters()` - Legacy interface

## Configuration

### Environment Variables
- Uses Reth IPC at `/tmp/reth.ipc`
- Outputs to vega-rs via ZMQ: `ipc:///tmp/vega_inbound`
- MEV-Share endpoint: `https://mev-share.flashbots.net`

### Price Cache
- LRU cache with configurable size (default: 10 entries)
- Prevents duplicate processing of identical price updates
- Thread-safe implementation for concurrent access

## Optimizations

### 1. Parallel Processing
- Concurrent handling of mempool and MEV-Share streams
- Asynchronous transaction processing pipeline
- Non-blocking price extraction and validation

### 2. Connection Resilience
```rust
const SECONDS_BEFORE_RECONNECTION: u64 = 2;
```
- Automatic reconnection on IPC failures
- Graceful handling of stream interruptions
- Continuous operation during network instability

### 3. Memory Efficiency
- Bounded caches prevent memory leaks
- Efficient bytecode scanning using sliding windows
- Minimal allocation in hot paths

## Data Flow

1. **Transaction Detection**: Monitor mempool/MEV-Share for pending txs
2. **Address Filtering**: Check if sender is tracked Chainlink forwarder
3. **Function Filtering**: Verify transaction calls `forward()` with `transmit()` data
4. **Price Extraction**: Decode nested calldata to extract new price
5. **Deduplication**: Check LRU cache to avoid reprocessing
6. **Forwarding**: Send `PriceUpdateBundle` to vega-rs via ZMQ

## Message Format

Sends `PriceUpdateBundle` containing:
```rust
pub struct PriceUpdateBundle {
    pub trace_id: String,           // Correlation ID for debugging
    pub tx_hash: String,            // Pending transaction hash
    pub raw_tx: Option<Bytes>,      // Raw transaction for replay
    pub inclusion_block: String,    // Target block for inclusion
    pub tx_new_price: U256,        // Extracted price value
    pub forward_to: Address,       // Chainlink aggregator address
    pub tx_from: Address,          // Transaction sender
    pub tx_to: Address,            // Transaction recipient
    pub tx_input: Bytes,           // Transaction calldata
}
```

## Error Handling

### Robust Processing
- Continues operation on individual transaction failures
- Comprehensive logging for debugging missed updates
- Graceful degradation when one data source fails

### Common Issues
1. **Missing Updates**: Usually due to untracked forwarder addresses
2. **Parsing Errors**: May indicate changes in Chainlink submission format
3. **Connection Issues**: Automatic reconnection with exponential backoff

## Performance Metrics

- **Latency**: ~100ms from mempool detection to vega-rs notification
- **Throughput**: Handles 1000+ transactions/second scanning
- **Memory**: ~50MB baseline with bounded growth
- **CPU**: Low overhead due to efficient filtering

## Building

```bash
cargo build --release -p oops-rs
```

## Running

```bash
# Via startup script (recommended)
./scripts/startup-rs.sh

# Direct execution
./target/release/oops-rs
```

## Debugging

### Check Address Tracking
```bash
# Verify forwarder addresses are current
grep "Forward" /var/log/overlord-rs/oops-rs.log

# Check for parsing errors
grep "parsing" /var/log/overlord-rs/oops-rs.log
```

### Monitor Price Updates
```bash
# Follow detected updates in real-time
tail -f /var/log/overlord-rs/oops-rs.log | grep "price update"
```

## Dependencies

- **alloy**: Ethereum library for RPC and types
- **mev-share-sse**: MEV-Share event stream client  
- **zmq**: High-performance messaging
- **lru**: Efficient caching
- **futures**: Async stream processing
