# profito-rs (The Liquidator)

The liquidation execution engine that calculates optimal liquidation parameters and submits profitable transactions via MEV-Share and Flashbots.

## Overview

profito-rs is the "profit engine" of overlord-rs. It receives underwater user alerts from vega-rs, replicates AAVE's liquidation logic in Rust for parameter calculation, determines the most profitable liquidation strategy, and crafts MEV bundles for submission to block builders.

## Architecture

```
┌─────────────┐
│   vega-rs   │
│(Underwater  │
│  Users)     │
└──────┬──────┘
       │ ZMQ
       ▼
┌─────────────┐    ┌──────────────────┐    ┌─────────────┐
│ Liquidation │    │  Flash Loan      │    │ MEV Bundle  │
│ Calculator  │───▶│  Optimizer       │───▶│  Creator    │
└─────────────┘    └──────────────────┘    └──────┬──────┘
                                                   │
                                                   ▼
                                           ┌─────────────┐
                                           │ Flashbots / │
                                           │ MEV-Share   │
                                           └─────────────┘
```

## Profit Calculation

Given an underwater user alert (HF < 1), profito-rs answers: _is this user profitable to liquidate?_

The net profitability calculation has three components:

**NetProfit = LiquidationBonus - DeterministicCosts - NonDeterministicCosts**

### 1. Liquidation Bonus Calculation

**LiquidationBonus = DebtRepaid × BonusMultiplier − DebtRepaid**

- `BonusMultiplier` comes from AAVE's liquidation bonus configuration (queryable via `AaveProtocolDataProvider`)
- `DebtRepaid` is determined by iterating over user's collateral reserves and calculating optimal liquidation amounts
- The system determines both _which_ reserve to liquidate and _how much_ based on health factor and close factor thresholds

### 2. Cost Components

**Deterministic Costs:**
- Gas fees: `GasUsed × (BaseFee + PriorityFee)`
- Flash loan fees: `DebtRepaid × FlashLoanRate`
- Slippage costs: `CollateralToReceive × SlippageRate`

**Non-Deterministic Costs:**
- MEV bribes to block builders
- Priority fees for transaction inclusion
- Network congestion adjustments

## Key Features

### 1. AAVE Logic Replication
Implements AAVE v3's liquidation calculation logic in Rust:

```rust
pub struct BestPair {
    pub collateral_asset: Address,           // Asset to liquidate
    pub debt_asset: Address,                 // Debt to repay
    pub net_profit: U256,                   // Expected profit
    pub actual_collateral_to_liquidate: U256, // Liquidation size
    pub actual_debt_to_liquidate: U256,     // Debt amount
    pub liquidation_protocol_fee_amount: U256, // AAVE fees
    pub flash_loan_source: Foxdie::FlashLoanSource, // Optimal source
}
```

### 2. Flash Loan Source Optimization
Intelligently selects the best liquidity source:

1. **Morpho Protocol**: First choice for gas efficiency
2. **AAVE Flash Loans**: Fallback with broader asset support  
3. **Custom Sources**: Additional protocols as needed

```rust
pub async fn get_best_liquidity_provider(
    debt_asset: Address,
    amount: U256,
) -> LiquiditySolution {
    // Check Morpho balance first (cheapest)
    if morpho_balance >= amount {
        return LiquiditySolution { source: MORPHO, .. };
    }
    
    // Fall back to AAVE if enabled
    if aave_flashloan_enabled {
        return LiquiditySolution { source: AAVE, .. };
    }
    
    // No suitable source found
    LiquiditySolution { source: NONE, .. }
}
```

### 3. Swap Fee Calculation
Calculates Uniswap V3 swap fees for accurate profit estimation:
```rust
pub async fn calculate_best_swap_fees(
    token_in: Address,
    token_out: Address, 
    amount_in: U256,
) -> Vec<(U24, U256)> // [(fee_tier, amount_out)]
```

## MEV Bundle Creation

### 1. Bundle Components
Each profitable liquidation generates a bundle containing:

1. **Price Update Transaction**: From oops-rs (when applicable)
2. **Liquidation Transaction**: Call to Foxdie contract
3. **Bribe Transaction**: Payment to block builder

### 2. Bribe Calculation
```rust
const BRIBE_IN_BASIS_POINTS: u16 = 9500; // 95% of profit
let bribe_amount = (net_profit * BRIBE_IN_BASIS_POINTS) / 10000;
```

### 3. MEV-Share Submission
```rust
// Submit bundle with privacy preferences
let bundle = BundleRequest {
    inclusion: InclusionRequest {
        block: target_block,
        max_block: target_block + 3, // 3 block window
    },
    body: transactions,
    privacy: Some(PrivacyHint {
        calldata: true,  // Hide transaction details
        contract_address: true,
        function_selector: true,
        logs: true,
    }),
};
```

## Optimization Strategies

### 1. Price Cache
Maintains cache of asset prices to avoid redundant oracle calls:
```rust
pub struct PriceCache {
    prices: HashMap<Address, (U256, Instant)>, // (price, timestamp)
    ttl: Duration, // Time to live for cached prices
}
```

### 2. Provider Connection Pooling
```rust
pub struct ProviderCache {
    connections: Vec<RootProvider<PubSubFrontend>>,
    current_index: AtomicUsize,
}
```

### 3. Concurrent Processing
Processes multiple liquidation opportunities in parallel:
```rust
// Handle multiple underwater users concurrently
let tasks: Vec<_> = underwater_events
    .into_iter()
    .map(|event| async move {
        process_liquidation_opportunity(event).await
    })
    .collect();

let results = join_all(tasks).await;
```

## Foxdie Contract Integration

profito-rs integrates with a custom liquidation contract (Foxdie) which handles:

1. **Flash Loan Management**: Handles multiple liquidity sources
2. **Atomic Liquidations**: Ensures all-or-nothing execution
3. **Profit Extraction**: Captures liquidation bonuses efficiently
4. **Gas Optimization**: Minimizes transaction costs

```rust
// Example Foxdie call
let foxdie_call = Foxdie::liquidateCall {
    asset: collateral_asset,
    debt_asset,
    user: underwater_user,
    debt_to_cover: optimal_debt_amount,
    receive_a_token: false, // Receive underlying asset
};
```

## Configuration

### Environment Variables
- `FOXDIE_ADDRESS`: Liquidation contract address
- `FOXDIE_OWNER_PK`: Private key for transaction signing
- `BUILDER_REGISTRATION_FILE_PATH`: MEV builder configurations

### Profitability Parameters
```rust
const MIN_PROFIT_THRESHOLD_USD: f64 = 10.0; // Minimum $10 profit
const MAX_GAS_PRICE_GWEI: u64 = 50; // Gas price limit
const BRIBE_PERCENTAGE: u16 = 95; // 95% of profit as bribe
```

## Building

```bash
cargo build --release -p profito-rs
```

## Running

```bash
# Via startup script (recommended)
./scripts/startup-rs.sh
```

## Development Tools

### bpchecker (Best Pair Checker)

A standalone analysis tool for validating liquidation opportunities and debugging calculations. It's best to
run it through the [bur.sh](../../bur.sh) script at the root of the repo.

**bpchecker** provides:
- **Validation**: Replicates exact AAVE v3 liquidation logic for verification
- **Analysis**: Detailed breakdown of all debt/collateral pair combinations  
- **Testing**: Generates forge commands for local Foxdie contract testing
- **Production**: Outputs cast commands for actual liquidation execution
- **Debugging**: Compares calculations with profito-rs automated results

This tool is essential for ensuring profito-rs calculations match AAVE's on-chain behavior.

## Dependencies

- **alloy**: Ethereum library for contract interactions
- **overlord-shared**: Common types and AAVE bindings
- **mev-share**: MEV-Share client for bundle submission
- **tokio**: Async runtime for concurrent processing
