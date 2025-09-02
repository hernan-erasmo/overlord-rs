# overlord-rs

A high-performance AAVE v3 liquidation bot built in Rust, designed to compete with professional MEV bots by leveraging advanced optimization techniques and real-time market monitoring.

## Architecture Overview

overlord-rs is a distributed system composed of five specialized components that work together to identify and execute profitable liquidations on AAVE v3:

```
┌─────────────┐    ┌──────────────────┐    ┌─────────────┐
│   oops-rs   │    │ whistleblower-rs │    │   vega-rs   │
│  (Oracle    │    │ (Event Listener) │    │   (Brain)   │
│  Scout)     │────┤                  │────┤             │
└─────────────┘    └──────────────────┘    └─────────────┘
                                                   │
                                                   ▼
                                           ┌─────────────┐
                                           │ profito-rs  │
                                           │(Liquidator) │
                                           └─────────────┘
                              ┌─────────────────┐
                              │ overlord-shared │
                              │                 │
                              │  (Common Utils) │
                              └─────────────────┘
```

### Key Components

- **[oops-rs](crates/oops-rs/README.md)** - Optimistic Oracle Price Scout: Monitors mempool for Chainlink price updates
- **[whistleblower-rs](crates/whistleblower-rs/README.md)** - Event listener for AAVE v3 protocol events affecting user health factors
- **[vega-rs](crates/vega-rs/README.md)** - Core calculation engine that maintains user health factor cache and identifies liquidation opportunities
- **[profito-rs](crates/profito-rs/README.md)** - Liquidation executor that calculates optimal parameters and submits transactions via Flashbots
- **[overlord-shared](crates/overlord-shared/README.md)** - Shared utilities and data structures

## Key Optimizations

1. **Mempool Monitoring**: Pre-emptively detects price updates before they hit the blockchain
2. **Smart Caching**: Maintains bucketed user caches, only recalculating affected positions
3. **ZMQ Communication**: High-performance inter-process communication between components
4. **Parallel Processing**: Concurrent health factor calculations using async Rust
5. **MEV-Share Integration**: Leverages private mempool for competitive advantage
6. **Flash Loan Optimization**: Intelligently selects between AAVE, Morpho, and other liquidity sources

## Quick Start

1. **Clone and build**:
   ```bash
   git clone <repo-url>
   cd overlord-rs
   cargo build --release --workspace
   ```

2. **Environment setup**:
   ```bash
   cp .env.example .env
   # Edit .env with your configuration
   ```

3. **Run the system**:
   ```bash
   ./scripts/startup-rs.sh
   ```

## Environment Variables

Key variables needed in `.env`:

- `FOXDIE_ADDRESS` - Your liquidation contract address
- `FOXDIE_OWNER_PK` - Private key for contract owner
- `VEGA_ADDRESSES_FILE` - File containing AAVE user addresses to monitor
- `VEGA_CHAINLINK_ADDRESSES_FILE` - Chainlink oracle mappings
- `TEMP_OUTPUT_DIR` - Directory for output files and logs

## Prerequisites

- Rust 1.70+
- Access to an Ethereum node (Reth recommended via IPC)
- MEV-Share access
- Deployed liquidation contract

## Performance Characteristics

- **Memory**: Efficiently handles 100k+ user addresses in cache
- **Latency**: Sub-second health factor recalculations
- **Throughput**: Processes multiple price updates and events concurrently
- **Reliability**: Automatic reconnection and error recovery

## Monitoring and Debugging

The system provides comprehensive logging:

- `/var/log/overlord-rs/` - Main log directory
- Structured logs with trace IDs for transaction correlation
- Health factor traces for debugging liquidation detection

Example log filtering:
```bash
# Find events from specific date
cat /var/log/overlord-rs/overlord-rs-processed.log | grep -n "2025-01-30" | head -n1 | cut -d: -f1

# Extract logs from line number
tail -n +66000 /var/log/overlord-rs/overlord-rs-processed.log > filtered.log
```

## Troubleshooting

### Missing Liquidations

If liquidations are being missed:

1. Check if `oops-rs` detected the price update
2. Verify Chainlink forwarder addresses are up to date
3. Check for parsing errors in logs
4. Ensure oracle address tracking is complete

### Performance Issues

- Monitor memory usage of user cache
- Check IPC connection stability
- Verify ZMQ message queue health
- Review concurrent task performance

## Development

Each component can be built and tested independently:

```bash
# Build specific component
cargo build --release -p vega-rs

# Run with debug logging
RUST_LOG=debug ./target/release/vega-rs
```

## License

[Add your license here]

## Contributing

[Add contribution guidelines here]
