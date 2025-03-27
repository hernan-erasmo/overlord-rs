use crate::constants::{AAVE_ORACLE_ADDRESS, AAVE_V3_POOL_ADDRESS, AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, AAVE_V3_PROVIDER_ADDRESS, UNISWAP_V3_FACTORY, UNISWAP_V3_QUOTER};
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
        UniswapV3Quoter, IAToken, ERC20, pool::AaveV3Pool, AaveUIPoolDataProvider, AaveProtocolDataProvider,
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

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/configuration/UserConfiguration.sol#L71
pub fn is_using_as_collateral_or_borrowing(user_config: U256, reserve_index: usize) -> bool {
    // In Solidity: (self.data >> (reserveIndex << 1)) & 3 != 0
    // This checks both collateral AND borrowing bits
    let shift_amount = reserve_index * 2; // reserveIndex << 1
    let shifted = user_config >> shift_amount;
    let mask = U256::from(3); // Binary: 11

    (shifted & mask) != U256::ZERO
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/configuration/UserConfiguration.sol#L103
pub fn is_using_as_collateral(user_config: U256, reserve_index: usize) -> bool {
    // In Solidity: (self.data >> ((reserveIndex << 1) + 1)) & 1 != 0
    // This checks only the collateral bit
    let shift_amount = (reserve_index * 2) + 1; // (reserveIndex << 1) + 1
    let shifted = user_config >> shift_amount;
    let mask = U256::from(1); // Binary: 1

    (shifted & mask) != U256::ZERO
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/configuration/UserConfiguration.sol#L87
pub fn is_borrowing(user_config: U256, reserve_index: usize) -> bool {
    // In Solidity: (self.data >> (reserveIndex << 1)) & 1 != 0
    // This checks only the borrowing bit
    let shift_amount = reserve_index * 2; // reserveIndex << 1
    let shifted = user_config >> shift_amount;
    let mask = U256::from(1); // Binary: 1

    (shifted & mask) != U256::ZERO
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/math/WadRayMath.sol#L47
pub fn wad_div(a: U256, b: U256) -> U256 {
    let wad: U256 = U256::from(10).pow(U256::from(18)); // 1e18
    let half_b = b / U256::from(2); // div(b, 2)

    // c = (a * WAD + halfB) / b
    let numerator = a * wad + half_b;
    numerator / b
}

/// TODO(Hernan): I'm not 100% sure that this function fetches a price internally. If it does, then you
/// must refactor it to use the price cache, otherwise results will be skewed when called from
/// underwater event processing at profito
async fn get_user_balance_in_base_currency(
    provider: Arc<RootProvider<PubSubFrontend>>,
    reserve: Address,
    a_token_address: Address,
    user_address: Address,
    asset_price: U256,
    asset_unit: U256,
) -> U256 {
    // Implementation of
    // https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/logic/GenericLogic.sol#L249
    let normalized_income = match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getReserveNormalizedIncome(reserve)
        .call()
        .await
    {
        Ok(response) => response._0,
        Err(e) => {
            eprintln!("Error trying to call getReserveNormalizedIncome: {}", e);
            U256::ZERO
        }
    };

    let a_token = IAToken::new(a_token_address, provider);
    // TODO(Hernan): revisit this, because scaledBalanceOf != balanceOf
    let scaled_balance = match a_token.scaledBalanceOf(user_address).call().await {
        Ok(balance_of_response) => balance_of_response._0,
        Err(e) => {
            eprintln!("Error trying to call balanceOf for {}: {}", user_address, e);
            U256::ZERO
        }
    };

    let balance = ray_mul(scaled_balance, normalized_income) * asset_price;
    balance / asset_unit
}

/// TODO(Hernan): I'm not 100% sure that this function fetches a price internally. If it does, then you
/// must refactor it to use the price cache, otherwise results will be skewed when called from
/// underwater event processing at profito
async fn get_user_debt_in_base_currency(
    provider: Arc<RootProvider<PubSubFrontend>>,
    reserve: Address,
    variable_debt_token_address: Address,
    user_address: Address,
    asset_price: U256,
    asset_unit: U256,
) -> U256 {
    // Implementation of
    // https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/logic/GenericLogic.sol#L219
    // TODO(Hernan): revisit this, because scaledBalanceOf != balanceOf
    let variable_debt_token = IAToken::new(variable_debt_token_address, provider.clone());
    let mut user_total_debt = match variable_debt_token
        .scaledBalanceOf(user_address)
        .call()
        .await
    {
        Ok(balance) => balance._0,
        Err(e) => {
            eprintln!("Error getting scaled debt balance: {}", e);
            return U256::ZERO;
        }
    };
    if user_total_debt == U256::ZERO {
        return U256::ZERO;
    }
    let normalized_debt = match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getReserveNormalizedVariableDebt(reserve)
        .call()
        .await
    {
        Ok(response) => response._0,
        Err(e) => {
            eprintln!("Error trying to call getReserveNormalizedDebt: {}", e);
            U256::ZERO
        }
    };
    user_total_debt = ray_mul(user_total_debt, normalized_debt) * asset_price;
    return user_total_debt / asset_unit;
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/math/WadRayMath.sol#L65
pub fn ray_mul(a: U256, b: U256) -> U256 {
    let ray: U256 = U256::from_str_radix("1000000000000000000000000000", 10).unwrap(); // 1e27
    let half_ray: U256 = U256::from_str_radix("500000000000000000000000000", 10).unwrap(); // 0.5e27

    if a == U256::ZERO || b == U256::ZERO {
        return U256::ZERO;
    }

    // c = (a * b + half_ray) / ray
    let product = a * b;
    let with_half = product + half_ray;
    with_half / ray
}

/// This is the equivalent of _calculateUserAccountData() in LiquidationLogic.sol
/// https://github.com/aave-dao/aave-v3-origin/blob/bb6ea42947f349fe8182a0ea30c5a7883d1f9ed1/src/contracts/protocol/libraries/logic/GenericLogic.sol#L63
/// except for emode support. We don't do that here.
pub async fn calculate_user_account_data(
    price_cache: Arc<tokio::sync::Mutex<PriceCache>>,
    provider: Arc<RootProvider<PubSubFrontend>>,
    user_address: Address,
    reserves_list: Vec<Address>,
    reserves_data: Vec<AggregatedReserveData>,
) -> Result<(U256, U256, U256), Box<dyn std::error::Error>> {
    // Capture required input arguments
    let mut total_collateral_in_base_currency = U256::ZERO;
    let mut total_debt_in_base_currency = U256::ZERO;
    let mut avg_liquidation_threshold = U256::ZERO;
    let user_config = match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getUserConfiguration(user_address)
        .call()
        .await
    {
        Ok(user_config) => user_config._0,
        Err(e) => {
            return Err(format!("Error trying to call getUserConfiguration: {}", e).into())
        }
    };

    // Operate
    for (i, reserve_address) in reserves_list.into_iter().enumerate() {
        if !is_using_as_collateral_or_borrowing(user_config.data, i) {
            continue;
        }

        /*
           Both reservesList and reservesData are aligned in the same order, meaning reservesList[i] is an asset
           address and reservesData[i] is the data for that asset.

           That assertion is what makes the following valid.
        */
        let liquidation_threshold = reserves_data[i].reserveLiquidationThreshold;
        let decimals = reserves_data[i].decimals;
        let asset_unit = U256::from(10).pow(U256::from(decimals));
        let asset_price = match price_cache.lock().await.get_price(reserve_address, None, AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone())).await {
            Ok(price) => price,
            Err(e) => {
                return Err(format!("Error trying to get price for {}: {}", reserve_address, e).into())
            }
        };

        // Calculate collateral totals
        if liquidation_threshold != U256::ZERO && is_using_as_collateral(user_config.data, i) {
            let user_balance_in_base_currency = get_user_balance_in_base_currency(
                provider.clone(),
                reserve_address,
                reserves_data[i].aTokenAddress,
                user_address,
                asset_price,
                asset_unit,
            )
            .await;
            total_collateral_in_base_currency += user_balance_in_base_currency;
            avg_liquidation_threshold += user_balance_in_base_currency * liquidation_threshold;
        };

        // Calculate debt totals
        if is_borrowing(user_config.data, i) {
            if match AaveProtocolDataProvider::new(
                AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS,
                provider.clone(),
            )
            .getIsVirtualAccActive(reserve_address)
            .call()
            .await
            {
                Ok(response) => response._0,
                Err(e) => {
                    return Err(format!("Error trying to call getIsVirtualAccActive: {}", e).into())
                }
            } {
                let user_debt_in_base_currency = get_user_debt_in_base_currency(
                    provider.clone(),
                    reserve_address,
                    reserves_data[i].variableDebtTokenAddress,
                    user_address,
                    asset_price,
                    asset_unit,
                )
                .await;
                total_debt_in_base_currency += user_debt_in_base_currency;
            } else {
                // custom case for GHO, which applies the GHO discount on balanceOf
                // https://github.com/aave-dao/aave-v3-origin/blob/bb6ea42947f349fe8182a0ea30c5a7883d1f9ed1/src/contracts/protocol/libraries/logic/GenericLogic.sol#L148
                total_debt_in_base_currency +=
                    match ERC20::new(reserves_data[i].variableDebtTokenAddress, provider.clone())
                        .balanceOf(user_address)
                        .call()
                        .await
                    {
                        Ok(balance_of_response) => balance_of_response.balance,
                        Err(e) => {
                            return Err(format!("Error trying to call balanceOf for {}: {}", user_address, e).into())
                        }
                    } * asset_price
                        / asset_unit;
            }
        }
    }

    if total_collateral_in_base_currency != U256::ZERO {
        avg_liquidation_threshold /= total_collateral_in_base_currency;
    } else {
        avg_liquidation_threshold = U256::ZERO;
    }

    let health_factor;
    if total_debt_in_base_currency != U256::ZERO {
        let wad_numerator =
            percent_mul(total_collateral_in_base_currency, avg_liquidation_threshold);
        health_factor = wad_div(wad_numerator, total_debt_in_base_currency);
    } else {
        health_factor = U256::MAX;
    }

    // Return values
    Ok(
        ( total_collateral_in_base_currency,
        total_debt_in_base_currency,
        health_factor)
    )
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
