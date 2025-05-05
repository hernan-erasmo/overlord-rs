use alloy::{
    primitives::{aliases::U24, utils::format_units, Address, U256},
    providers::{Provider, RootProvider},
    pubsub::PubSubFrontend,
};
use overlord_shared::{
    constants::{
        AAVE_ORACLE_ADDRESS, AAVE_V3_POOL_ADDRESS, AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, MORPHO,
        UNISWAP_V3_FACTORY, WETH,
    },
    sol_bindings::{
        pool::AaveV3Pool,
        AaveOracle, AaveProtocolDataProvider, Foxdie, IAToken,
        IUiPoolDataProviderV3::{AggregatedReserveData, UserReserveData},
        UniswapV3Factory, UniswapV3Pool, ERC20,
    },
};
use std::sync::Arc;

use super::cache::PriceCache;
use tracing::warn;

pub const BRIBE_IN_BASIS_POINTS: u16 = 9500; // 95%

#[derive(Clone, Debug)]
pub struct BestPair {
    pub collateral_asset: Address,
    pub debt_asset: Address,
    pub net_profit: U256,
    pub printable_net_profit: String,
    pub actual_collateral_to_liquidate: U256,
    pub actual_debt_to_liquidate: U256,
    pub liquidation_protocol_fee_amount: U256,
    pub flash_loan_source: Foxdie::FlashLoanSource,
}

#[derive(Clone, Debug)]
pub struct LiquiditySolution {
    pub source: Foxdie::FlashLoanSource,
    pub reasons: Vec<String>,
}

pub async fn get_best_liquidity_provider(
    provider: Arc<RootProvider<PubSubFrontend>>,
    debt_asset: Address,
    actual_debt_to_liquidate: U256,
) -> LiquiditySolution {
    let mut reasons = vec![];

    // Query MORPHO's balanceOf asset that we'll need to borrow
    let morpho_balance = match ERC20::new(debt_asset, provider.clone())
        .balanceOf(MORPHO)
        .call()
        .await
    {
        Ok(balance_of_response) => balance_of_response.balance,
        Err(e) => {
            let error_msg = format!("Error trying to call balanceOf for {}: {}", debt_asset, e);
            warn!("{}", error_msg);
            reasons.push(error_msg);
            return LiquiditySolution {
                source: Foxdie::FlashLoanSource::NONE,
                reasons,
            };
        }
    };

    // If MORPHO's balance is enough, then we don't need to continue processing
    if morpho_balance >= actual_debt_to_liquidate {
        return LiquiditySolution {
            source: Foxdie::FlashLoanSource::MORPHO,
            reasons,
        };
    } else {
        reasons.push(format!(
            "MORPHO balance for {} ({}) is not enough",
            debt_asset, morpho_balance
        ));
    }

    let pool_data_provider =
        AaveProtocolDataProvider::new(AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let is_flashloan_enabled = match pool_data_provider
        .getFlashLoanEnabled(debt_asset)
        .call()
        .await
    {
        Ok(res) => res._0,
        Err(e) => {
            let error_msg = format!(
                "Error trying to determine if AAVE flashloan is enabled for {}: {}",
                debt_asset, e
            );
            warn!("{}", error_msg.clone());
            reasons.push(error_msg);
            return LiquiditySolution {
                source: Foxdie::FlashLoanSource::NONE,
                reasons,
            };
        }
    };

    if !is_flashloan_enabled {
        reasons.push(format!("AAVE flashLoan is not enabled for {}", debt_asset));
        return LiquiditySolution {
            source: Foxdie::FlashLoanSource::NONE,
            reasons,
        };
    };

    // The process to query AAVE v3 balances is a little more indirect. First we need to get the
    // AToken contract address corresponding to the underlying we want to borrow:
    let a_token_debt_address = match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getReserveData(debt_asset)
        .call()
        .await
    {
        Ok(reserve_data) => reserve_data._0.aTokenAddress,
        Err(e) => {
            let error_msg = format!("Couldn't get reserve data for calculating best flash loan provider for debt {}: {}", debt_asset, e);
            warn!("{}", error_msg.clone());
            reasons.push(error_msg);
            return LiquiditySolution {
                source: Foxdie::FlashLoanSource::NONE,
                reasons,
            };
        }
    };

    // Now we query the asset's balanceOf of the AToken contract
    let aave_balance = match ERC20::new(debt_asset, provider.clone())
        .balanceOf(a_token_debt_address)
        .call()
        .await
    {
        Ok(balance_of_response) => balance_of_response.balance,
        Err(e) => {
            let error_msg = format!("Error trying to call balanceOf for {}: {}", debt_asset, e);
            warn!("{}", error_msg.clone());
            reasons.push(error_msg);
            return LiquiditySolution {
                source: Foxdie::FlashLoanSource::NONE,
                reasons,
            };
        }
    };

    if aave_balance >= actual_debt_to_liquidate {
        return LiquiditySolution {
            source: Foxdie::FlashLoanSource::AAVE_V3,
            reasons,
        };
    } else {
        reasons.push(format!(
            "AAVE V3 balance for {} ({}) is not enough",
            debt_asset, aave_balance
        ));
    }

    LiquiditySolution {
        source: Foxdie::FlashLoanSource::NONE,
        reasons,
    }
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
    // is higher than this and, if it is, then it uses this value instead.
    // We can't check for that here because we're working the other way around (that is, we first do the calculations
    // and THEN we determine the inputs to the liquidationCall)
    max_liquidatable_debt
}

pub async fn calculate_user_balances(
    reserves_data: Vec<AggregatedReserveData>,
    supplied_reserve: &UserReserveData,
    borrowed_reserve: &UserReserveData,
    provider: Arc<RootProvider<PubSubFrontend>>,
    user_address: Address,
) -> Result<(AggregatedReserveData, U256, AggregatedReserveData, U256), Box<dyn std::error::Error>>
{
    let collateral_reserve = reserves_data
        .iter()
        .find(|agg_reserve_data| {
            agg_reserve_data.underlyingAsset == supplied_reserve.underlyingAsset
        })
        .unwrap();
    let collateral_a_token = IAToken::new(collateral_reserve.aTokenAddress, provider.clone());
    let user_collateral_balance = match collateral_a_token.balanceOf(user_address).call().await {
        Ok(response) => response._0,
        Err(e) => {
            return Err(format!("Error trying to call collateralAToken.balanceOf(): {}", e).into())
        }
    };
    let debt_reserve = reserves_data
        .iter()
        .find(|agg_reserve_data| {
            agg_reserve_data.underlyingAsset == borrowed_reserve.underlyingAsset
        })
        .unwrap();
    let debt_reserve_token = ERC20::new(debt_reserve.variableDebtTokenAddress, provider.clone());
    let user_reserve_debt = match debt_reserve_token.balanceOf(user_address).call().await {
        Ok(response) => response.balance,
        Err(e) => {
            return Err(format!("Error trying to call debt_reserve_token.balanceOf: {}", e).into())
        }
    };
    Ok((
        collateral_reserve.clone(),
        user_collateral_balance,
        debt_reserve.clone(),
        user_reserve_debt,
    ))
}

pub async fn get_reserves_list(
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<Vec<Address>, Box<dyn std::error::Error>> {
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
        Err(e) => Err(format!("Error trying to call getReservesList: {}", e).into()),
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
    user_total_debt / asset_unit
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
    trace_id: Option<String>,
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
        Err(e) => return Err(format!("Error trying to call getUserConfiguration: {}", e).into()),
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
        let asset_price = match price_cache
            .lock()
            .await
            .get_price(
                reserve_address,
                trace_id.clone(),
                AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone()),
            )
            .await
        {
            Ok(price) => price,
            Err(e) => {
                return Err(
                    format!("Error trying to get price for {}: {}", reserve_address, e).into(),
                )
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
                            return Err(format!(
                                "Error trying to call balanceOf for {}: {}",
                                user_address, e
                            )
                            .into())
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
    Ok((
        total_collateral_in_base_currency,
        total_debt_in_base_currency,
        health_factor,
    ))
}

/// Returns pools, fees and liquidity sorted by liquidity descending
async fn get_uniswap_v3_pools(
    provider: &RootProvider<PubSubFrontend>,
    token_a: Address,
    token_b: Address,
) -> Vec<(Address, U24, u128)> {
    let factory = UniswapV3Factory::new(UNISWAP_V3_FACTORY, provider.clone());
    let fee_tiers = [
        U24::from(100),
        U24::from(500),
        U24::from(3000),
        U24::from(10000),
    ]; // 0.01%, 0.05%, 0.3%, 1%
    let mut pools = Vec::new();

    for &fee in &fee_tiers {
        let pool_address = match factory.getPool(token_a, token_b, fee).call().await {
            Ok(response) => response._0,
            Err(e) => {
                println!("(Error fetching pool: {})", e);
                Address::ZERO
            }
        };
        let pool = UniswapV3Pool::new(pool_address, provider.clone());
        let in_range_liquidity = match pool.liquidity().call().await {
            Ok(response) => response._0,
            Err(e) => {
                println!("(Error fetching pool liquidity: {})", e);
                0
            }
        };
        if pool_address != Address::ZERO {
            pools.push((pool_address, fee, in_range_liquidity));
        }
    }

    pools.sort_by(|a, b| b.2.cmp(&a.2));
    pools
}

/// UniswapV3 fees are hundredths of basis points: 1% == 10000; 0,3% == 3000; 0,05% == 500; 0,01% == 100
/// Calculate and return the lowest fee tier for which there's enough liquidity
pub async fn calculate_best_swap_fees(
    provider: Arc<RootProvider<PubSubFrontend>>,
    collateral_asset: Address,
    debt_asset: Address,
) -> (U24, U24) {
    // collateral to weth, weth to debt
    let mut best_fees = (U24::from(10000), U24::from(10000));

    // Get collateral -> WETH pools
    if collateral_asset != WETH {
        let collateral_pools = get_uniswap_v3_pools(&provider, collateral_asset, WETH).await;
        println!(
            "\t\tFound {} collateral/WETH pools:",
            collateral_pools.len()
        );
        for (addr, fee, in_range_liquidity) in &collateral_pools {
            println!(
                "\t\t- Liquidity {} with {}hbps fee at pool {}",
                in_range_liquidity, fee, addr
            );
        }
        best_fees.0 = collateral_pools[0].1;
    } else {
        // If collateral is WETH, foxdie won't swap this leg because we're already where we want
        // (that is, holding WETH balance and right before swapping that WETH for debt asset to repay loan)
        // so it'll ignore whatever we send as COLLATERAL_TO_WETH_FEE
        best_fees.0 = U24::from(0);
        println!("\t\tCollateral is WETH, no need to swap this leg");
    }

    // Get WETH -> debt pools
    if debt_asset != WETH {
        let debt_pools = get_uniswap_v3_pools(&provider, WETH, debt_asset).await;
        println!("\t\tFound {} WETH/debt pools:", debt_pools.len());
        for (addr, fee, in_range_liquidity) in &debt_pools {
            println!(
                "\t\t- Liquidity {} with {}hbps fee at pool {}",
                in_range_liquidity, fee, addr
            );
        }
        best_fees.1 = debt_pools[0].1;
    } else {
        // Since foxdie swaps collateral for WETH irregardles of what the debt asset is, if it happens to be WETH
        // we're already where we want (that is, holding WETH after foxide swapped collateral for it, and right before
        // repaying the loan which, in this case, was WETH). Foxdie will ignore whatever we send as WETH_TO_DEBT_FEE
        best_fees.1 = U24::from(0);
        println!("\t\tDebt is WETH, no need to swap this leg");
    }

    // TODO: Calculate best fees based on liquidity and price impact
    best_fees
}

/// TODO: This function is not returning the appropriate amount of gas and needs
/// to be fixed.
pub async fn estimate_gas(provider: Arc<RootProvider<PubSubFrontend>>) -> (U256, U256, U256) {
    let default_gas_used = U256::from(700000);
    let gas_price_in_gwei = match provider.get_gas_price().await {
        Ok(price) => U256::from(price) / U256::from(1e3),
        _ => U256::MAX,
    };
    (
        default_gas_used,
        gas_price_in_gwei,
        default_gas_used * gas_price_in_gwei / U256::from(1000000),
    )
    /*
    match Foxdie::new(FOXDIE_ADDRESS, provider.clone())
        .triggerLiquidation(Foxdie::LiquidationParams {
            debtAmount: U256::ZERO,
            user: Address::ZERO,
            debtAsset: Address::ZERO,
            collateral: Address::ZERO,
            collateralToWethFee: U24::from(0),
            wethToDebtFee: U24::from(0),
            bribePercentBps: BRIBE_IN_BASIS_POINTS,
            flashLoanSource: Foxdie::FlashLoanSource::MORPHO,
            aavePremium: U256::ZERO,
        }).estimate_gas().await {
            Ok(gas_used) => {
                (U256::from(gas_used), gas_price_in_gwei, U256::from(gas_used) * gas_price_in_gwei / U256::from(1000000))
            },
            Err(e) => {
                println!("Error estimating gas: {}", e);
                warn!("Error estimating gas: {}", e);
                return (default_gas_used, gas_price_in_gwei, default_gas_used * gas_price_in_gwei / U256::from(1000000))
            }
        }
     */
}

/// This function is supposed to be the EXACT SAME copy of the one defined
/// in bpchecker, with the only difference being the removal of print statements
/// and different error handling. Logic MUST BE THE SAME. The problem is that
/// I don't have time to refactor those out now.
///
/// In the future, if you have time, try to figure out a way of adding a debug
/// mode, or something like that, so that the print statements are only executed
/// when called from bpchecker, and then you'll be able to remove this duplicate logic
async fn calculate_available_collateral_to_liquidate(
    provider: Arc<RootProvider<PubSubFrontend>>,
    collateral_asset: Address,
    // all original args for this function under this line
    collateral_asset_price: U256,
    collateral_asset_unit: U256,
    debt_asset_price: U256,
    debt_asset_unit: U256,
    debt_to_cover: U256,
    user_collateral_balance: U256,
    liquidation_bonus: U256,
) -> Result<(U256, U256, U256, U256), Box<dyn std::error::Error>> {
    // Implementation of
    // https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L633

    let protocol =
        AaveProtocolDataProvider::new(AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let liquidation_protocol_fee_percentage = match protocol
        .getLiquidationProtocolFee(collateral_asset)
        .call()
        .await
    {
        Ok(response) => response._0,
        Err(e) => {
            return Err(format!("Error trying to call collateralAToken.balanceOf(): {}", e).into())
        }
    };
    let base_collateral = (debt_asset_price * debt_to_cover * collateral_asset_unit)
        / (collateral_asset_price * debt_asset_unit);
    let max_collateral_to_liquidate = percent_mul(base_collateral, liquidation_bonus);

    let mut collateral_amount;
    let debt_amount_needed;
    if max_collateral_to_liquidate > user_collateral_balance {
        collateral_amount = user_collateral_balance;
        debt_amount_needed = percent_div(
            (collateral_asset_price * collateral_amount * debt_asset_unit)
                / (debt_asset_price * collateral_asset_unit),
            liquidation_bonus,
        );
    } else {
        collateral_amount = max_collateral_to_liquidate;
        debt_amount_needed = debt_to_cover;
    }

    let mut liquidation_protocol_fee = U256::ZERO;
    if liquidation_protocol_fee_percentage != U256::ZERO {
        let bonus_collateral =
            collateral_amount - percent_div(collateral_amount, liquidation_bonus);
        liquidation_protocol_fee =
            percent_mul(bonus_collateral, liquidation_protocol_fee_percentage);
        collateral_amount -= liquidation_protocol_fee;
    }

    // THIS IS THE CORE OF THE CALCULATION, WHAT DECIDES WHETHER OR NOT WE MOVE ON WITH THE EXECUTION
    // this section doesn't belong to the original solidity function
    let debt_in_collateral_units = (debt_amount_needed * debt_asset_price * collateral_asset_unit)
        / (collateral_asset_price * debt_asset_unit);
    // This already has the liquidation fee deducted
    let base_profit = if collateral_amount >= debt_in_collateral_units {
        collateral_amount - debt_in_collateral_units
    } else {
        debt_in_collateral_units - collateral_amount
    };

    let execution_gas_cost = estimate_gas(provider.clone()).await.2;
    // this assumes we will swap in 1% fee pools (could be more sophisticated)
    // uniswap v3 fees are represented as hundredths of basis points: 1% == 100; 0,3% == 30; 0,05% == 5; 0,01% == 1
    let swap_loss_factor = U256::from(100);
    let swap_total_cost = percent_mul(collateral_amount, swap_loss_factor);
    let total_cost = execution_gas_cost + swap_total_cost;

    // this will cause some weird numbers in output logs for positions with a single possible
    // pair, but would make sure no overflow errors accidentally replace the best pair
    // when there are more than one combination
    let net_profit = if total_cost > base_profit {
        U256::MIN
    } else {
        base_profit - total_cost
    };
    Ok((
        collateral_amount,
        debt_amount_needed,
        liquidation_protocol_fee,
        (net_profit * collateral_asset_price) / collateral_asset_unit,
    ))
}

/// Returns the appropriate bribe based on the amount earned
pub fn calculate_bribe() -> U256 {
    // From 0 to 9999
    U256::from(BRIBE_IN_BASIS_POINTS)
}

/// Not exactly the same as the one from bpchecker
/// The biggest difference between this one and the one from bpchecker.rs is the
/// way they deal with prices (this one, from the price cache, and the one from bpchecker,
/// from the fork itself)
/// This function also assumes trace_id will always be NOT none, as opposed to the one in bpchecker
/// which passes none since it's only for compatibility purposes with the price cache integration
/// in some of the helper functions.
pub async fn get_best_liquidation_opportunity(
    user_reserve_data: Vec<UserReserveData>, // for borrowed_reserve and supplied_reserve
    reserves_data: Vec<AggregatedReserveData>,
    user_address: Address,
    health_factor_v33: U256,
    total_debt_in_base_currency: U256,
    price_cache: Arc<tokio::sync::Mutex<PriceCache>>,
    provider: Arc<RootProvider<PubSubFrontend>>,
    trace_id: String,
    oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, Arc<RootProvider<PubSubFrontend>>>,
) -> Option<BestPair> {
    let mut best_pair: Option<BestPair> = None;
    for borrowed_reserve in user_reserve_data
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::ZERO)
    {
        for supplied_reserve in user_reserve_data
            .iter()
            .filter(|r| r.usageAsCollateralEnabledOnUser && r.scaledATokenBalance > U256::ZERO)
        {
            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L234-L238
            let (collateral_reserve, user_collateral_balance, debt_reserve, user_reserve_debt) =
                match calculate_user_balances(
                    reserves_data.clone(),
                    supplied_reserve,
                    borrowed_reserve,
                    provider.clone(),
                    user_address,
                )
                .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        warn!("Error calculating user balances: {}", e);
                        continue;
                    }
                };
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L234-L238

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L252-L276
            // TODO(Hernan): you should at least visually check if liquidationBonus is returning what you're expecting, since
            // the solidity implementation uses bit masking to get the value.
            let liquidation_bonus = collateral_reserve.reserveLiquidationBonus;
            let collateral_asset_price = price_cache
                .lock()
                .await
                .get_price(
                    supplied_reserve.underlyingAsset,
                    Some(trace_id.clone()),
                    oracle.clone(),
                )
                .await
                .unwrap();
            let debt_asset_price = price_cache
                .lock()
                .await
                .get_price(
                    borrowed_reserve.underlyingAsset,
                    Some(trace_id.clone()),
                    oracle.clone(),
                )
                .await
                .unwrap();
            let collateral_asset_unit = U256::from(10).pow(collateral_reserve.decimals);
            let debt_asset_unit = U256::from(10).pow(debt_reserve.decimals);
            let user_reserve_debt_in_base_currency =
                user_reserve_debt * debt_asset_price / debt_asset_unit;
            let user_reserve_collateral_in_base_currency =
                user_collateral_balance * collateral_asset_price / collateral_asset_unit;
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L252-L276

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L278-L302
            let actual_debt_to_liquidate = calculate_actual_debt_to_liquidate(
                user_reserve_debt,
                user_reserve_collateral_in_base_currency,
                user_reserve_debt_in_base_currency,
                health_factor_v33,
                total_debt_in_base_currency,
                debt_asset_unit,
                debt_asset_price,
            );
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L278-L302

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L309
            let (
                actual_collateral_to_liquidate,
                actual_debt_to_liquidate,
                liquidation_protocol_fee_amount,
                // net_profit comes denominated in base units,
                // comparable across different assets:
                //      (net_profit * collateral_asset_price) / collateral_asset_unit,
                net_profit,
            ) = match calculate_available_collateral_to_liquidate(
                provider.clone(),
                collateral_reserve.underlyingAsset,
                collateral_asset_price,
                collateral_asset_unit,
                debt_asset_price,
                debt_asset_unit,
                actual_debt_to_liquidate,
                user_collateral_balance,
                liquidation_bonus,
            )
            .await
            {
                Ok(result) => result,
                Err(e) => {
                    warn!("Error calculating available collateral to liquidate: {}", e);
                    continue;
                }
            };
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L309

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L320-L344
            // TODO(Hernan): do we need to make sure this doesn't bite us in the ass?
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L320-L344

            let printable_net_profit =
                format_units(net_profit, 8).unwrap_or_else(|_| "CONVERSION_ERROR".to_string());
            let best_liquidity_provider = get_best_liquidity_provider(
                provider.clone(),
                debt_reserve.underlyingAsset,
                actual_debt_to_liquidate,
            )
            .await;
            if net_profit > best_pair.as_ref().map_or(U256::ZERO, |p| p.net_profit)
                && best_liquidity_provider.source != Foxdie::FlashLoanSource::NONE
            {
                best_pair = Some(BestPair {
                    collateral_asset: supplied_reserve.underlyingAsset,
                    debt_asset: borrowed_reserve.underlyingAsset,
                    net_profit,
                    printable_net_profit,
                    actual_collateral_to_liquidate,
                    actual_debt_to_liquidate,
                    liquidation_protocol_fee_amount,
                    flash_loan_source: best_liquidity_provider.source,
                });
            }
        }
    }
    best_pair
}
