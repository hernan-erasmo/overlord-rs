# overlord-shared

Common utilities, data structures, and smart contract bindings shared across all overlord-rs components.

## Overview

overlord-shared provides the foundational infrastructure that enables seamless communication and data sharing between the different components of overlord-rs. It contains shared types, utility functions, contract bindings, and constants used throughout the system.

## Key Components

### 1. Message Types
Core data structures for inter-component communication:

```rust
// Price update from oops-rs to vega-rs
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

// Underwater user from vega-rs to profito-rs
pub struct UnderwaterUserEvent {
    pub address: Address,
    pub trace_id: String,
    pub tx_hash: Option<String>,
    pub raw_tx: Option<Bytes>,
    pub inclusion_block: String,
    pub total_collateral_base: U256,
    pub user_account_data: AaveV3Pool::getUserAccountDataReturn,
    pub new_asset_prices: Vec<(Address, String, U256)>,
}

// Event update from whistleblower-rs to vega-rs
pub struct WhistleblowerUpdate {
    pub trace_id: String,
    pub details: WhistleblowerEventDetails,
}
```

### 2. Smart Contract Bindings
Auto-generated Rust bindings for key protocols:

#### AAVE v3 Contracts
```rust
// Core AAVE v3 contracts
pub mod pool;                    // Main AAVE v3 Pool
pub struct AaveOracle;           // Price oracle
pub struct AaveProtocolDataProvider; // Protocol data
pub struct AaveUIPoolDataProvider;   // UI helper

// Token contracts
pub struct ERC20;                // Standard ERC20
pub struct IAToken;              // AAVE interest-bearing tokens
```

#### Chainlink Contracts
```rust
pub struct AccessControlledOCR2Aggregator; // Price aggregators
pub struct AuthorizedForwarder;            // Price update forwarders
pub struct EACAggregatorProxy;             // Proxy contracts
```

#### DeFi Protocols
```rust
pub struct UniswapV3Factory;    // Uniswap V3 factory
pub struct UniswapV3Pool;       // Uniswap V3 pools
pub struct Foxdie;              // Custom liquidation contract
```

### 3. Common Utilities
Shared functionality across components:

```rust
// Reserve data fetching
pub async fn get_reserves_data(
    provider: &RootProvider<PubSubFrontend>
) -> Result<Vec<AggregatedReserveData>, Box<dyn std::error::Error>>;

// Price conversions and calculations
pub fn normalize_price(price: U256, decimals: u8) -> U256;
pub fn calculate_usd_value(amount: U256, price: U256, decimals: u8) -> U256;
```

### 4. Constants and Addresses
Centralized configuration for protocol addresses:

```rust
// Core AAVE v3 addresses
pub const AAVE_V3_POOL_ADDRESS: Address = 
    address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2");
pub const AAVE_ORACLE_ADDRESS: Address = 
    address!("54586bE62E3c3580375aE3723C145253060Ca0C2");

// DeFi protocol addresses  
pub const UNISWAP_V3_FACTORY: Address = 
    address!("1F98431c8aD98523631AE4a59f267346ea31F984");
pub const MORPHO: Address = 
    address!("9994E35Db50125E0DF82e4c2dde5628E71f8d2");

// Special oracle addresses
pub const GHO_PRICE_ORACLE: Address = 
    address!("3f12643d3f6f874d39c2a4c9f2cd6f2dbac877fc");

// ZMQ endpoints
pub const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";
```

## Architecture Benefits

### 1. Type Safety
- Compile-time verification of data structures
- Prevents data corruption between components
- Clear API contracts between modules

### 2. Code Reuse
- Single source of truth for contract ABIs
- Shared utility functions reduce duplication
- Consistent data handling across components

### 3. Maintainability
- Centralized contract address management
- Easy updates to shared functionality
- Version compatibility across components

## Contract Binding Generation

Smart contract bindings are generated using `alloy-sol-macro`:

```rust
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    #[allow(clippy::too_many_arguments)]
    AAVE_V3_POOL,
    "abis/aave_v3_pool.json"
);
```

This provides:
- Type-safe contract calls
- Automatic encoding/decoding
- Event filtering and parsing
- Error handling

## Message Serialization

All inter-component messages use efficient serialization:

```rust
// Bincode for high-performance binary serialization
#[derive(Serialize, Deserialize)]
pub struct MessageBundle {
    pub bundle_type: BundleType,
    pub data: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub enum BundleType {
    PriceUpdate,
    UnderwaterUser,
    WhistleblowerEvent,
}
```

## Error Handling

Standardized error types across components:

```rust
#[derive(Debug)]
pub enum OverlordError {
    ContractCall(String),
    Serialization(String),
    Network(String),
    Configuration(String),
}

impl std::error::Error for OverlordError {}
```

## Configuration Management

Centralized configuration helpers:

```rust
pub fn get_required_env_var(key: &str) -> Result<String, OverlordError> {
    env::var(key).map_err(|_| {
        OverlordError::Configuration(format!("Missing required env var: {}", key))
    })
}

pub fn load_addresses_from_file(path: &str) -> Result<Vec<Address>, OverlordError> {
    // Load and parse address files with error handling
}
```

## Testing Utilities

Helper functions for component testing:

```rust
#[cfg(test)]
pub mod test_utils {
    pub fn create_mock_price_update() -> PriceUpdateBundle { ... }
    pub fn create_mock_underwater_user() -> UnderwaterUserEvent { ... }
    pub fn setup_test_provider() -> RootProvider<PubSubFrontend> { ... }
}
```

## Building

```bash
cargo build --release -p overlord-shared
```

## Usage in Components

### In oops-rs:
```rust
use overlord_shared::{PriceUpdateBundle, MessageBundle};

let bundle = PriceUpdateBundle {
    trace_id: generate_trace_id(),
    tx_hash: tx.hash.to_string(),
    // ... other fields
};
```

### In vega-rs:
```rust
use overlord_shared::{
    sol_bindings::pool::AaveV3Pool,
    UnderwaterUserEvent,
    common::get_reserves_data,
};

let pool = AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider);
let user_data = pool.getUserAccountData(user_address).call().await?;
```

### In profito-rs:
```rust
use overlord_shared::{
    sol_bindings::{AaveOracle, UniswapV3Factory},
    constants::{AAVE_ORACLE_ADDRESS, UNISWAP_V3_FACTORY},
};

let oracle = AaveOracle::new(AAVE_ORACLE_ADDRESS, provider);
let asset_price = oracle.getAssetPrice(asset).call().await?;
```

## ABI Management

Contract ABIs are stored in the `abis/` directory:
- `aave_v3_pool.json` - Core AAVE pool contract
- `aave_oracle.json` - AAVE price oracle
- `uniswap_v3_factory.json` - Uniswap V3 factory
- `foxdie.json` - Custom liquidation contract

## Performance Considerations

### 1. Efficient Serialization
- Bincode for binary serialization (faster than JSON)
- Minimal allocations in hot paths
- Zero-copy deserialization where possible

### 2. Connection Reuse
- Shared provider instances across calls
- Connection pooling utilities
- Automatic retry mechanisms

### 3. Memory Management
- Bounded data structures
- Efficient collection types
- Minimal cloning in message passing

## Dependencies

- **alloy**: Ethereum types and contract bindings
- **serde**: Serialization framework
- **bincode**: Binary serialization format
- **chrono**: Date/time handling
- **tracing**: Structured logging

## Future Enhancements

1. **Dynamic ABI Loading**: Runtime contract discovery
2. **Multi-chain Support**: Abstract chain-specific addresses
3. **Advanced Caching**: Distributed cache for shared state
4. **Monitoring Integration**: Built-in metrics collection
