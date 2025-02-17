use alloy::{
    primitives::{aliases::U24, utils::format_units, Address, U160, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
};
use std::sync::Arc;
use crate::constants::{UNISWAP_V3_FACTORY, UNISWAP_V3_QUOTER};

use super::{
    cache::PriceCache,
    sol_bindings::{
        AaveOracle,
        IUiPoolDataProviderV3::UserReserveData,
        UniswapV3Factory,
        UniswapV3Quoter,
        UniswapV3Pool,
    },
    utils::ReserveConfigurationData,
};
use tracing::warn;

pub struct DebtCollateralPairInfo {
    pub debt_asset: Address,
    pub debt_symbol: String,
    pub debt_amount: U256,
    pub debt_in_collateral_units: U256,
    pub collateral_asset: Address,
    pub collateral_symbol: String,
    pub collateral_amount: U256,
    pub net_profit: String,
}

pub struct BestFeeTierForSwapResult {
    pub best_fee: U24,
    pub best_output: U256,
}

/// This mimics `percentMul` at
/// https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/math/PercentageMath.sol#L25
pub fn percent_mul(value: U256, percentage: U256) -> U256 {
    (value * percentage + U256::from(0.5e4)) / U256::from(1e4)
}

/// This mimics `percentDiv` at
/// https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/math/PercentageMath.sol#L48
pub fn percent_div(value: U256, percentage: U256) -> U256 {
    ((value * U256::from(1e4)) + (percentage / U256::from(2))) / percentage
}

/// Calculates the best fee tier to call the swap. Since the smart contract uses
/// _swapExactInputSingle(), then the "best" poolFee, is going to be the one that
/// provides the required liquidity for the lowest fee.
pub async fn get_best_fee_tier_for_swap(
    provider: RootProvider<PubSubFrontend>,
    token_in: Address,
    token_out: Address,
    amount: U256,
) -> BestFeeTierForSwapResult {
    let mut best_output = U256::ZERO;
    let mut best_fee = U24::from(100);
    let available_fees = vec![U24::from(100), U24::from(500), U24::from(3000), U24::from(10000)];

    let factory = UniswapV3Factory::new(UNISWAP_V3_FACTORY, provider.clone());
    let quoter = UniswapV3Quoter::new(UNISWAP_V3_QUOTER, provider.clone());

    for available_fee in available_fees.iter() {
        println!("\t\t\tChecking fee {}", available_fee);
        // Check if pool exists
        let pool_contract_address = match factory.getPool(token_in, token_out, *available_fee).call().await {
            Ok(address) => {
                if address._0 == Address::ZERO {
                    println!("\t\t\tPool doesn't exist for fee {}", available_fee);
                    continue; // Pool doesn't exist for this fee tier
                } else {
                    println!("\t\t\tFound pool for fee {} at {}", available_fee, address._0);
                }
                address._0
            },
            Err(e) => {
                // When running this against a local provider, you need to keep in mind pruning because that has already happened
                warn!("Failed to get pool address for fee {}: {}", available_fee, e);
                continue;
            }
        };

        // Need to instantiate the pool_contract_address to get the token0 and token1 values here
        let pool_contract = UniswapV3Pool::new(pool_contract_address, provider.clone());
        macro_rules! call_pool {
            ($method:ident) => {
                match pool_contract.$method().call().await {
                    Ok(val) => val._0,
                    Err(e) => {
                        println!("\t\t\t\tFailed to get {} for pool {}: {}", stringify!($method), pool_contract_address, e);
                        warn!("Failed to get {} for pool {}: {}", stringify!($method), pool_contract_address, e);
                        continue;
                    }
                }
            };
        }

        let token_0 = call_pool!(token0);
        let token_1 = call_pool!(token1);
        let fee = call_pool!(fee);

        // Get quote
        let output = match quoter
            .quoteExactInputSingle(
                token_0,
                token_1,
                fee,
                amount,
                U160::from(0), // sqrtPriceLimitX96
            )
            .call()
            .await
        {
            Ok(quoter_output) => {
                println!(
                    "\t\t\t\tOutput for token0: {}, token1: {}, and fee: {} - {}",
                    token_0, token_1, available_fee, quoter_output.amountOut
                );
                quoter_output.amountOut
            },
            Err(e) => {
                println!(
                    "\t\t\t\tFailed to get output for token0: {}, token1: {}, and fee: {}: {}",
                    token_0, token_1, available_fee, e
                );
                U256::from(0)
            }
        };

        if output > best_output {
            best_output = output;
            best_fee = *available_fee;
        }
    }

    BestFeeTierForSwapResult {
        best_fee,
        best_output,
    }
}

pub async fn get_best_debt_collateral_pair(
    candidate: Address,
    reserves_configuration: ReserveConfigurationData,
    user_reserve_data: Vec<UserReserveData>,
    user_health_factor: U256,
    price_cache: Arc<tokio::sync::Mutex<PriceCache>>,
    trace_id: String,
    oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, Arc<RootProvider<PubSubFrontend>>>,
) -> Option<DebtCollateralPairInfo> {
    let mut best_pair: Option<DebtCollateralPairInfo> = None;
    let mut max_net_profit = U256::from(0);
    let mut liquidation_close_factor;
    for borrowed_reserve in user_reserve_data
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::ZERO)
    {
        for supplied_reserve in user_reserve_data
            .iter()
            .filter(|r| r.usageAsCollateralEnabledOnUser && r.scaledATokenBalance > U256::ZERO)
        {
            if let (Some(debt_config), Some(collateral_config)) = (
                reserves_configuration.get(&borrowed_reserve.underlyingAsset),
                reserves_configuration.get(&supplied_reserve.underlyingAsset),
            ) {
                let debt_symbol = &debt_config.symbol;
                let collateral_symbol = &collateral_config.symbol;

                // source: https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L50-L68
                if user_health_factor <= U256::from(0.95e18) {
                    liquidation_close_factor = U256::from(1e4);
                } else {
                    liquidation_close_factor = U256::from(0.5e4);
                };

                // https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L379
                let mut actual_debt_to_liquidate = percent_mul(
                    borrowed_reserve.scaledVariableDebt,
                    liquidation_close_factor,
                );

                let collateral_asset_price = match price_cache
                    .lock()
                    .await
                    .get_price(
                        supplied_reserve.underlyingAsset,
                        trace_id.clone(),
                        oracle.clone(),
                    )
                    .await
                {
                    Ok(price) => price,
                    Err(e) => {
                        warn!("Failed to get collateral price: {}", e);
                        return None;
                    }
                };

                let debt_asset_price = match price_cache
                    .lock()
                    .await
                    .get_price(
                        borrowed_reserve.underlyingAsset,
                        trace_id.clone(),
                        oracle.clone(),
                    )
                    .await
                {
                    Ok(price) => price,
                    Err(e) => {
                        warn!("Failed to get debt price: {}", e);
                        return None;
                    }
                };

                let debt_asset_decimals = debt_config.data.decimals.to::<u8>();
                let collateral_asset_decimals = collateral_config.data.decimals.to::<u8>();

                let debt_asset_unit = U256::from(10).pow(U256::from(debt_asset_decimals));
                let collateral_asset_unit =
                    U256::from(10).pow(U256::from(collateral_asset_decimals));

                let base_collateral =
                    (debt_asset_price * actual_debt_to_liquidate * collateral_asset_unit)
                        / (collateral_asset_price * debt_asset_unit);
                // Yes, the liquidation bonus considered here is an attribute of the collateral asset. The following traces from here
                // https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L498
                // to
                // https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L146
                let max_collateral_to_liquidate =
                    percent_mul(base_collateral, collateral_config.data.liquidationBonus);

                let collateral_amount: U256;
                if max_collateral_to_liquidate > supplied_reserve.scaledATokenBalance {
                    collateral_amount = supplied_reserve.scaledATokenBalance;
                    actual_debt_to_liquidate = percent_div(
                        (collateral_asset_price * collateral_amount * debt_asset_unit)
                            / (debt_asset_price * collateral_asset_unit),
                        collateral_config.data.liquidationBonus,
                    );
                } else {
                    collateral_amount = max_collateral_to_liquidate;
                }

                let bonus_collateral;
                let mut liquidation_fee = U256::ZERO;
                if collateral_config.liquidation_fee != U256::ZERO {
                    bonus_collateral = collateral_amount
                        - percent_div(collateral_amount, collateral_config.data.liquidationBonus);
                    liquidation_fee =
                        percent_mul(bonus_collateral, collateral_config.liquidation_fee);
                }

                /*
                    Now, at this point, we already know the values of everything returned by _calculateAvailableCollateralToLiquidate(),

                    (
                        actualCollateralToLiquidate,  # collateral_amount - liquidation_protocol_fee
                        actualDebtToLiquidate,        # in this code, this is debt_amount_needed
                        liquidationProtocolFeeAmount  # in this code, this is liquidation_fee
                    )
                */

                let actual_collateral_to_liquidate = collateral_amount - liquidation_fee;

                // These aren't relevant to AAVE, that's why you won't find anything on them in Aave code
                // In order to calculate net profit, everthing must be denominated in collateral units
                // otherwise it will return garbage
                let debt_in_collateral_units =
                    (actual_debt_to_liquidate * debt_asset_price * collateral_asset_unit)
                        / (collateral_asset_price * debt_asset_unit);

                if collateral_amount < liquidation_fee + debt_in_collateral_units {
                    warn!(
                        "net profit for liquidation of user {} would've overflowed (debt {}/ collateral {})",
                        candidate,
                        debt_symbol,
                        collateral_symbol,
                    );
                    continue;
                }

                // THIS IS WHAT WE MUST OPTIMIZE FOR
                let net_profit = actual_collateral_to_liquidate - debt_in_collateral_units;
                if net_profit > max_net_profit {
                    max_net_profit = net_profit;
                    best_pair = Some(DebtCollateralPairInfo {
                        debt_asset: borrowed_reserve.underlyingAsset,
                        debt_symbol: debt_symbol.clone(),
                        debt_amount: actual_debt_to_liquidate,
                        debt_in_collateral_units,
                        collateral_symbol: collateral_symbol.clone(),
                        collateral_amount,
                        collateral_asset: supplied_reserve.underlyingAsset,
                        net_profit: format_units(
                            net_profit * collateral_asset_price,
                            collateral_asset_decimals + 8,
                        )
                        .unwrap(),
                    });
                }
            }
        }
    }
    best_pair
}
