use crate::constants::{AAVE_V3_POOL_ADDRESS, AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, AAVE_V3_PROVIDER_ADDRESS, UNISWAP_V3_FACTORY, UNISWAP_V3_QUOTER};
use alloy::{
    primitives::{aliases::U24, Address, U160, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
};
use std::sync::Arc;

use super::{
    cache::PriceCache,
    sol_bindings::{
        AaveOracle, IUiPoolDataProviderV3::{AggregatedReserveData, UserReserveData}, UniswapV3Factory, UniswapV3Pool,
        UniswapV3Quoter, IAToken, ERC20, pool::AaveV3Pool, AaveUIPoolDataProvider,
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

pub struct BestFlashSwapArgs {
    pub fee: U24,
    pub collateral_required: U256,
    pub pool: Address,
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
    token_debt: Address,
    token_collateral: Address,
    amount: U256,
) -> BestFlashSwapArgs {
    let mut best_output = U256::MAX;
    let mut best_fee = U24::from(100);
    let mut best_contract = Address::ZERO;
    let available_fees = [
        U24::from(100),   // 0.01%
        U24::from(500),   // 0.05%
        U24::from(3000),  // 0.3%
        U24::from(10000), // 1%
    ];

    let factory = UniswapV3Factory::new(UNISWAP_V3_FACTORY, provider.clone());
    let quoter = UniswapV3Quoter::new(UNISWAP_V3_QUOTER, provider.clone());

    for available_fee in available_fees.iter() {
        // Check if pool exists
        let pool_contract_address = match factory
            .getPool(token_debt, token_collateral, *available_fee)
            .call()
            .await
        {
            Ok(address) => {
                if address._0 == Address::ZERO {
                    println!("\t\t\tPool doesn't exist for fee {}", available_fee);
                    continue; // Pool doesn't exist for this fee tier
                }
                address._0
            }
            Err(e) => {
                // When running this against a local provider, you need to keep in mind pruning because that has already happened
                warn!(
                    "Failed to get pool address for fee {}: {}",
                    available_fee, e
                );
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
                        warn!(
                            "Failed to get {} for pool {}: {}",
                            stringify!($method),
                            pool_contract_address,
                            e
                        );
                        continue;
                    }
                }
            };
        }

        let fee = call_pool!(fee);

        // Get quote
        let output = match quoter
            .quoteExactOutputSingle(token_collateral, token_debt, fee, amount, U160::from(0))
            .call()
            .await
        {
            Ok(quote) => {
                println!(
                    "\t\t\tPool {} requires {} collateral to repay {} debt",
                    pool_contract_address, quote.amountIn, amount
                );
                quote.amountIn
            }
            _ => {
                continue;
            }
        };

        if output < best_output {
            best_output = output;
            best_fee = *available_fee;
            best_contract = pool_contract_address;
        }
    }

    BestFlashSwapArgs {
        fee: best_fee,
        collateral_required: best_output,
        pool: best_contract,
    }
}

pub fn calculate_actual_debt_to_liquidate(
    user_reserve_debt: U256,
    user_reserve_collateral_in_base_currency: U256,
    user_reserve_debt_in_base_currency: U256,
    health_factor_v33: U256,
    total_debt_in_base_currency: U256,
    debt_asset_unit: U256,
    debt_asset_price: U256,
) -> U256 {
    let MIN_BASE_MAX_CLOSE_FACTOR_THRESHOLD = U256::from(2000e8);
    let CLOSE_FACTOR_HF_THRESHOLD = U256::from(0.95e18);
    let DEFAULT_LIQUIDATION_CLOSE_FACTOR = U256::from(0.5e4);

    // by default whole debt in the reserve could be liquidated
    let mut max_liquidatable_debt = user_reserve_debt;

    // but if debt and collateral are above or equal MIN_BASE_MAX_CLOSE_FACTOR_THRESHOLD
    // and health factor is above CLOSE_FACTOR_HF_THRESHOLD this amount may be adjusted
    if user_reserve_collateral_in_base_currency >= MIN_BASE_MAX_CLOSE_FACTOR_THRESHOLD
        && user_reserve_debt_in_base_currency >= MIN_BASE_MAX_CLOSE_FACTOR_THRESHOLD
        && health_factor_v33 >= CLOSE_FACTOR_HF_THRESHOLD
    {
        let total_default_liquidatable_debt_in_base_currency = percent_mul(
            total_debt_in_base_currency,
            DEFAULT_LIQUIDATION_CLOSE_FACTOR,
        );

        // if the debt is more than the DEFAULT_LIQUIDATION_CLOSE_FACTOR % of the whole,
        // then we CAN liquidate only up to DEFAULT_LIQUIDATION_CLOSE_FACTOR %
        if user_reserve_debt_in_base_currency > total_default_liquidatable_debt_in_base_currency {
            max_liquidatable_debt = (total_default_liquidatable_debt_in_base_currency
                * debt_asset_unit)
                / debt_asset_price;
            println!("\t\tv3.3 partial max liquidatable debt (total_d * debt_unit) / debt_price = {} * {} / {} = {}", total_default_liquidatable_debt_in_base_currency, debt_asset_unit, debt_asset_price, max_liquidatable_debt);
        }
    }

    // in solidity, there's a check that verifies if what the user send as debtToCover on the liquidationCall
    // is higher than this and, if it is, then it uses this value instead. We don't care about that because we'll
    // always want to liquidate as much as possible.
    max_liquidatable_debt
}

pub async fn calculate_user_balances(
    reserves_data: Vec<AggregatedReserveData>,
    supplied_reserve: &UserReserveData,
    borrowed_reserve: &UserReserveData,
    provider: Arc<RootProvider<PubSubFrontend>>,
    user_address: Address,
) -> Result<(AggregatedReserveData, U256, AggregatedReserveData, U256), Box<dyn std::error::Error>> {
    let collateral_reserve = reserves_data
        .iter()
        .find(|agg_reserve_data| {
            agg_reserve_data.underlyingAsset == supplied_reserve.underlyingAsset
        })
        .unwrap();
    let collateral_a_token =
        IAToken::new(collateral_reserve.aTokenAddress, provider.clone());
    let user_collateral_balance =
        match collateral_a_token.balanceOf(user_address).call().await {
            Ok(response) => response._0,
            Err(e) => return Err(format!("Error trying to call collateralAToken.balanceOf(): {}", e).into())
        };
    let debt_reserve = reserves_data
        .iter()
        .find(|agg_reserve_data| {
            agg_reserve_data.underlyingAsset == borrowed_reserve.underlyingAsset
        })
        .unwrap();
    let debt_reserve_token =
        ERC20::new(debt_reserve.variableDebtTokenAddress, provider.clone());
    let user_reserve_debt = match debt_reserve_token.balanceOf(user_address).call().await {
        Ok(response) => response.balance,
        Err(e) => return Err(format!("Error trying to call debt_reserve_token.balanceOf: {}", e).into())
    };
    Ok((collateral_reserve.clone(), user_collateral_balance, debt_reserve.clone(), user_reserve_debt))
}

pub async fn get_reserves_list(provider: Arc<RootProvider<PubSubFrontend>>) -> Result<Vec<Address>, Box<dyn std::error::Error>> {
    /*
       According to https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/pool/Pool.sol#L532
       the reserves list is ordered the same way as the _reserveList storage in the Pool contract.
    */
    match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getReservesList()
        .call()
        .await
    {
        Ok(reserves_list) => Ok(reserves_list._0),
        Err(e) => Err(format!("Error trying to call getReservesList: {}", e).into())
    }
}

pub async fn get_reserves_data(provider: Arc<RootProvider<PubSubFrontend>>) -> Result<Vec<AggregatedReserveData>, Box<dyn std::error::Error>> {
    /*
       According to https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/helpers/UiPoolDataProviderV3.sol#L45
       the reserves data is ordered the same way as the reserves list (it actually calls pool.getReservesList() and uses it as index)
    */
    match AaveUIPoolDataProvider::new(AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, provider.clone())
        .getReservesData(AAVE_V3_PROVIDER_ADDRESS)
        .call()
        .await
    {
        Ok(reserves_data) => Ok(reserves_data._0),
        Err(e) => Err(format!("Error trying to call getReservesData: {}", e).into())
    }
}

/// Not exactly the same as the one from bpchecker
/// The biggest difference between this one and the one from bpchecker.rs is the
/// way they deal with prices (this one, from the price cache, and the one from bpchecker,
/// from the fork itself)
pub async fn get_best_liquidation_opportunity(
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

    for borrowed_reserve in user_reserve_data
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::ZERO)
    {
        for supplied_reserve in user_reserve_data
            .iter()
            .filter(|r| r.usageAsCollateralEnabledOnUser && r.scaledATokenBalance > U256::ZERO)
        {

        }
    }







    // TODO: Implement the rest of the function












    best_pair
}
