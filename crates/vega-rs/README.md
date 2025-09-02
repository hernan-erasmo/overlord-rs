# vega-rs (The Brain)

The core calculation engine and orchestration center of overlord-rs that maintains user health factor caches and identifies liquidation opportunities in real-time.

## Overview

vega-rs is the "brain" of the overlord-rs system. It ingests all AAVE v3 user addresses, maintains an intelligent cache of their health factors, and efficiently recalculates only affected positions when price updates or protocol events occur. This smart caching strategy enables sub-second liquidation detection across 100k+ addresses.

## Architecture

```
┌─────────────┐    ┌──────────────────┐
│   oops-rs   │───▶│                  │
│Price Updates│    │   Smart Cache    │◀──┐
└─────────────┘    │   Management     │   │
                   │                  │   │
┌─────────────┐    │                  │   │
│whistleblower│───▶│                  │   │
│   -rs       │    └──────────┬───────┘   │
│Events       │               │           │
└─────────────┘               ▼           │
                   ┌──────────────────┐   │
                   │Health Factor     │   │
                   │Calculator        │   │
                   └──────────┬───────┘   │
                              │           │
                              ▼ ZMQ       │
                   ┌──────────────────┐   │
                   │   profito-rs     │   │
                   │ (Liquidations)   │   │
                   └──────────────────┘   │
                                          │
                   ┌──────────────────┐   │
                   │ User Addresses   │───┘
                   │    File Input    │
                   └──────────────────┘
```

## Key Features

### 1. Intelligent User Cache
The system maintains a sophisticated two-tier cache:

```rust
// Cache Structure: Reserve -> PositionType -> [Users]
HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>>

enum PositionType {
    Borrowed,    // Users borrowing this asset
    Collateral,  // Users supplying this asset as collateral
}
```

### 2. Smart Recalculation Strategy

**Price Updates**: Only recalculate users affected by specific assets
```rust
// Example: USDC price update only affects:
// - Users borrowing USDC (debt value changes)
// - Users with USDC collateral (collateral value changes)
let affected_users = cache.get_users_for_reserve(usdc_address);
```

**Protocol Events**: Targeted updates for specific users
```rust
// Example: User borrows ETH -> only update that user's health factor
match event_type {
    WhistleblowerEventType::Borrow => {
        update_single_user(event.user, event.reserve);
    }
}
```

### 3. Parallel Processing
```rust
// Concurrent health factor calculations across user buckets
const BUCKETS: usize = 64;
let tasks: Vec<_> = address_buckets
    .into_iter()
    .map(|bucket| async move {
        calculate_health_factors(bucket).await
    })
    .collect();
```

## Data Flow

### Initialization
1. **Address Loading**: Read user addresses from data files
2. **Reserve Discovery**: Query AAVE for all available reserves
3. **Cache Population**: Build position mappings for all users
4. **Initial Scan**: Calculate baseline health factors

### Runtime Operations
1. **Listen for Updates**: Receive price updates from oops-rs and events from whistleblower-rs
2. **Impact Analysis**: Determine which users are affected
3. **Targeted Recalculation**: Update only affected health factors
4. **Liquidation Detection**: Identify users with HF < 1.0
5. **Event Broadcasting**: Send `UnderwaterUserEvent` to profito-rs

## Optimizations

### 1. Bucketed Processing
Users are divided into buckets for parallel processing:
```rust
const BUCKETS: usize = 64;  // Configurable bucket count
let buckets = addresses.chunks(addresses.len() / BUCKETS);
```

### 2. Minimum Thresholds
Filters out unprofitable positions:
```rust
const MIN_COLLATERAL_THRESHOLD_IN_USD: f64 = 6.0;
const MIN_REPORTABLE_COLLATERAL: f64 = 1e10; // $10+ USD equivalent
```

### 3. Health Factor Caching
- In-memory cache of calculated health factors
- Delta updates instead of full recalculations
- Efficient storage using `RwLock<HashMap>`

### 4. Fork Provider Optimization
Custom provider implementation optimized for high-frequency calls:
```rust
pub struct ForkProvider {
    // Optimized for repeated AAVE contract calls
    // Connection pooling and request batching
}
```

## Cache Management

### User Position Tracking
```rust
struct UserPosition {
    scaled_atoken_balance: U256,           // Collateral amount
    usage_as_collateral_enabled_on_user: bool, // Collateral flag
    scaled_variable_debt: U256,            // Debt amount  
    underlying_asset: ReserveAddress,      // Asset address
}
```

### Cache Updates
- **Price Updates**: Bulk update all users of affected assets
- **User Events**: Single user position updates
- **Periodic Refresh**: Full cache rebuild (configurable interval)

## Message Processing

### Input Messages
1. **PriceUpdateBundle** from oops-rs:
   ```rust
   // Triggers recalculation for users of affected asset
   let affected_reserves = chainlink_to_reserves(bundle.forward_to);
   ```

2. **WhistleblowerUpdate** from whistleblower-rs:
   ```rust
   // Updates specific user's position in cache
   cache.update_user_position(update.user, update.reserve);
   ```

### Output Messages
Sends `UnderwaterUserEvent` to profito-rs:
```rust
pub struct UnderwaterUserEvent {
    pub address: Address,
    pub trace_id: String,               // Correlation tracking
    pub tx_hash: Option<String>,        // Triggering transaction
    pub raw_tx: Option<Bytes>,         // Raw transaction data
    pub inclusion_block: String,        // Target block
    pub total_collateral_base: U256,   // Total collateral value
    pub user_account_data: AaveV3Pool::getUserAccountDataReturn,
    pub new_asset_prices: Vec<(Address, String, U256)>, // Price context
}
```

## Configuration

### Environment Variables
- `VEGA_ADDRESSES_FILE`: User addresses to monitor
- `VEGA_CHAINLINK_ADDRESSES_FILE`: Oracle mapping configuration
- `TEMP_OUTPUT_DIR`: Output directory for health factor traces

### Command Line Options
```bash
vega-rs --buckets 64  # Adjust parallel processing buckets
```

## Performance Characteristics

### Memory Usage
- **Baseline**: ~500MB for 100k users
- **Growth**: Linear with user count
- **Cache Size**: Configurable per asset type

### Processing Speed
- **Full Scan**: ~30 seconds for 100k users
- **Price Update**: <1 second for affected subset
- **Single User**: <10ms per calculation

### Throughput
- **Price Updates**: 10+ per second sustained
- **User Events**: 100+ per second burst capability
- **Health Factor Calculations**: 1000+ users/second

## Building

```bash
cargo build --release -p vega-rs
```

## Running

```bash
# Via startup script (recommended)
./scripts/startup-rs.sh

# Direct execution with custom bucket count
./target/release/vega-rs --buckets 128
```

## Monitoring

### Health Factor Traces
```bash
# View health factor calculations for specific trace
cat .temp_output/hf-traces/{trace_id}.txt

# Monitor underwater users
tail -f .temp_output/init_hf_under_1_results_*.txt
```

### Performance Metrics
```bash
# Processing times per update
grep "elapsed_ms" /var/log/overlord-rs/vega-rs.log

# Cache hit rates
grep "cache" /var/log/overlord-rs/vega-rs.log | grep "hit\|miss"
```

### User Statistics
```bash
# Most active reserves
grep "most_.*_reserve" /var/log/overlord-rs/vega-rs.log
```

## Debugging

### Cache State
```bash
# Verify cache population
grep "cache.*initialized" /var/log/overlord-rs/vega-rs.log

# Check for cache misses
grep "user not found in cache" /var/log/overlord-rs/vega-rs.log
```

### Price Update Processing
```bash
# Track price update handling
grep "PriceUpdateBundle" /var/log/overlord-rs/vega-rs.log

# Monitor affected user counts
grep "affected.*users" /var/log/overlord-rs/vega-rs.log
```

## Dependencies

- **alloy**: Ethereum provider and contract interactions
- **overlord-shared**: Common types and utilities
- **tokio**: Async runtime and concurrency
- **zmq**: High-performance messaging
- **futures**: Parallel async processing

## Advanced Features

### 1. Historical Analysis
- Exports health factor traces for analysis
- Maintains audit trail of all calculations
- Supports replay for debugging liquidation misses

### 2. Dynamic Reconfiguration
- Hot-reload of user address lists
- Runtime adjustment of processing parameters
- Dynamic reserve addition/removal

### 3. Failover Support
- Graceful degradation on provider failures
- Automatic cache reconstruction
- State persistence across restarts
