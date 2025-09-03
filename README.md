# overlord-rs

A high-performance AAVE v3 liquidation bot built in Rust, designed to compete with professional MEV bots by leveraging advanced optimization techniques and real-time market monitoring.

## Real-World Performance Examples

**ETH Price Update Liquidation** - 1.5 seconds from mempool price update capture to bundle submission:

```plaintext
2025-07-15T11:56:19.385381462-03:00  INFO oops_rs: MEMPOOL update sent. trace_id=45dff5ff expected_block=22925317 tx_hash=0x45dff5ffb5071b1dc9d464de107ad1e9e35c5796a6fc3c749c103dace32ce227 slot_info=(7.4, 4.6) price=297854000000 forward_to=0x7d4E742018fb52E48b08BE73d041C18B21de6Fb5
2025-07-15T11:56:19.385589837-03:00  INFO vega_rs: Vega received price update for trace_id 45dff5ff
2025-07-15T11:56:19.390692201-03:00  INFO vega_rs::user_reserve_cache: Candidates ready for analysis trace_id=45dff5ff processing_time_ms=5 total_candidates=16418 unique_candidates=13437 buckets=[1120, 1120, 1120, 1120, 1120, 1120, 1120, 1120, 1120, 1119, 1119, 1119] asset_details=(osETH, 203), (WETH, 12430), (cbETH, 177), (wstETH, 2379), (rETH, 363), (weETH, 748), (ETHx, 60), (rsETH, 58)
2025-07-15T11:56:19.536749007-03:00  INFO vega_rs::fork_provider: Anvil fork started at block 22925316 for bundle 45dff5ff
2025-07-15T11:56:19.537702042-03:00  INFO vega_rs::fork_provider: Successfuly set storage for bundle 45dff5ff
2025-07-15T11:56:19.537712475-03:00  INFO vega_rs::fork_provider: Storage in fork for bundle 45dff5ff has been tweaked
2025-07-15T11:56:19.537805012-03:00  INFO vega_rs::calc_utils: About to start HF calculation tasks for bundle 45dff5ff
2025-07-15T11:56:20.723874738-03:00  INFO vega_rs: ALERT (from event bus) | 45dff5ff | 0x6872E3B7C26F9Df50f1a6121CD97206ECc5bF7e8 has HF < 1: 997817374504651381 (total collateral 48300093430)
2025-07-15T11:56:20.72868879-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for WETH (new value = 297854000000)
2025-07-15T11:56:20.7300433-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for wstETH (new value = 297854000000)
2025-07-15T11:56:20.730054929-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for cbETH (new value = 297854000000)
2025-07-15T11:56:20.73006166-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for rETH (new value = 297854000000)
2025-07-15T11:56:20.730066591-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for weETH (new value = 297854000000)
2025-07-15T11:56:20.730071084-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for osETH (new value = 297854000000)
2025-07-15T11:56:20.730075761-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for ETHx (new value = 297854000000)
2025-07-15T11:56:20.73008135-03:00  INFO profito_rs::cache::price: Successfully override 45dff5ff price cache for rsETH (new value = 297854000000)
2025-07-15T11:56:20.730087165-03:00  INFO profito_rs::cache::price: Dropping prices cached for fdb093ea
2025-07-15T11:56:20.792032671-03:00  INFO profito_rs: liquidate 0x6872E3B7C26F9Df50f1a6121CD97206ECc5bF7e8 @ 45dff5ff for $13.88107341 (total collateral 48300093430)
2025-07-15T11:56:21.87602611-03:00  INFO profito_rs: Submitted bundle. Response: SendBundleResponse { bundle_hash: 0xd9a244e2009137d05820e06991dfdb91caec7ddc637f8b6949676c7b08fea9d6 }
2025-07-15T11:56:26.323814012-03:00  INFO vega_rs: Candidates analysis complete for 45dff5ff | 6938 ms | 13437 candidates processed in 12 buckets | 8 with HF < 1
```

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

This project is licensed under the Creative Commons Attribution-NonCommercial 4.0 International License (CC BY-NC 4.0).

You are free to:
- **Share** — copy and redistribute the material in any medium or format
- **Adapt** — remix, transform, and build upon the material

Under the following terms:
- **Attribution** — You must give appropriate credit, provide a link to the license, and indicate if changes were made
- **NonCommercial** — You may not use the material for commercial purposes

For more details, see the [full license text](https://creativecommons.org/licenses/by-nc/4.0/).

## Contributing

Open to PRs, issues, comments, etc. I'll take a look whenever I can.
