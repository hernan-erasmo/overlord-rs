use alloy::{
    primitives::{aliases::U24, utils::format_units, Address, U256},
    providers::{IpcConnect, Provider, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
};
use profito_rs::cache::PriceCache;
use profito_rs::{
    calculations::{percent_div, percent_mul, calculate_actual_debt_to_liquidate, calculate_user_balances, get_reserves_list, get_reserves_data},
    constants::{
        AAVE_ORACLE_ADDRESS, AAVE_V3_POOL_ADDRESS, AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, UNISWAP_V3_FACTORY, WETH,
    },
    sol_bindings::{
        pool::AaveV3Pool,
        AaveOracle, AaveProtocolDataProvider, IAToken,
        IUiPoolDataProviderV3::{AggregatedReserveData, UserReserveData},
        UniswapV3Factory, UniswapV3Pool, ERC20,
    },
    utils::{ReserveConfigurationEnhancedData, generate_reserve_details_by_asset, get_user_reserves_data},
};
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::Mutex;

#[derive(Debug)]
struct BestPair {
    collateral_asset: Address,
    debt_asset: Address,
    net_profit: U256,
    actual_collateral_to_liquidate: U256,
    actual_debt_to_liquidate: U256,
    liquidation_protocol_fee_amount: U256,
}

async fn get_user_health_factor(provider: Arc<RootProvider<PubSubFrontend>>, user: Address) -> U256 {
    let pool = AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone());
    match pool.getUserAccountData(user).call().await {
        Ok(account_data) => account_data.healthFactor,
        Err(e) => {
            eprintln!("Error trying to call getUserAccountData: {}", e);
            std::process::exit(1);
        }
    }
}

/// This function is the rust equivalent of _calculateDebt() defined in LiquidationLogic.sol
/// https://github.com/aave/aave-v3-core/blob/b74526a7bc67a3a117a1963fc871b3eb8cea8435/contracts/protocol/libraries/logic/LiquidationLogic.sol#L363
async fn calculate_debt(
    provider: Arc<RootProvider<PubSubFrontend>>,
    user: Address,
    debt_asset: Address,
    user_health_factor: U256,
) -> (U256, U256, U256) {
    let pool = AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone());
    let stable_debt_token_address: Address;
    let variable_debt_token_address: Address;
    (stable_debt_token_address, variable_debt_token_address) =
        match pool.getReserveData(debt_asset).call().await {
            Ok(reserve_data) => (
                reserve_data._0.stableDebtTokenAddress,
                reserve_data._0.variableDebtTokenAddress,
            ),
            Err(e) => {
                eprintln!("Error trying to call getUserAccountData: {}", e);
                std::process::exit(1);
            }
        };
    let stable_debt = ERC20::new(stable_debt_token_address, provider.clone());
    let variable_debt = ERC20::new(variable_debt_token_address, provider.clone());
    let user_stable_debt = match stable_debt.balanceOf(user).call().await {
        Ok(balance_of_return) => balance_of_return.balance,
        Err(e) => {
            eprintln!("Failed to get stable debt balance: {}", e);
            U256::ZERO
        }
    };
    let user_variable_debt = match variable_debt.balanceOf(user).call().await {
        Ok(balance_of_return) => balance_of_return.balance,
        Err(e) => {
            eprintln!("Failed to get variable debt balance: {}", e);
            U256::ZERO
        }
    };
    let user_total_debt = user_stable_debt + user_variable_debt;
    let close_factor = if user_health_factor <= U256::from(0.95e18) {
        U256::from(1e4)
    } else {
        U256::from(0.5e4)
    };

    let max_liquidatable_debt = percent_mul(user_total_debt, close_factor);

    // The solidity function does one more step, and instead of returning max_liquidatable_debt, it checks
    // if that value is above or below whatever the user called liquidationCall with. This is what makes the "pass
    // uint(-1) to liquidate max available" possible. We don't need to do that here, as we are only interested in
    // the amount of debt that can be liquidated.
    (user_variable_debt, user_total_debt, max_liquidatable_debt)
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/configuration/UserConfiguration.sol#L71
fn is_using_as_collateral_or_borrowing(user_config: U256, reserve_index: usize) -> bool {
    // In Solidity: (self.data >> (reserveIndex << 1)) & 3 != 0
    // This checks both collateral AND borrowing bits
    let shift_amount = reserve_index * 2; // reserveIndex << 1
    let shifted = user_config >> shift_amount;
    let mask = U256::from(3); // Binary: 11

    (shifted & mask) != U256::ZERO
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/configuration/UserConfiguration.sol#L103
fn is_using_as_collateral(user_config: U256, reserve_index: usize) -> bool {
    // In Solidity: (self.data >> ((reserveIndex << 1) + 1)) & 1 != 0
    // This checks only the collateral bit
    let shift_amount = (reserve_index * 2) + 1; // (reserveIndex << 1) + 1
    let shifted = user_config >> shift_amount;
    let mask = U256::from(1); // Binary: 1

    (shifted & mask) != U256::ZERO
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/configuration/UserConfiguration.sol#L87
fn is_borrowing(user_config: U256, reserve_index: usize) -> bool {
    // In Solidity: (self.data >> (reserveIndex << 1)) & 1 != 0
    // This checks only the borrowing bit
    let shift_amount = reserve_index * 2; // reserveIndex << 1
    let shifted = user_config >> shift_amount;
    let mask = U256::from(1); // Binary: 1

    (shifted & mask) != U256::ZERO
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/math/WadRayMath.sol#L65
fn ray_mul(a: U256, b: U256) -> U256 {
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

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/math/WadRayMath.sol#L47
fn wad_div(a: U256, b: U256) -> U256 {
    let wad: U256 = U256::from(10).pow(U256::from(18)); // 1e18
    let half_b = b / U256::from(2); // div(b, 2)

    // c = (a * WAD + halfB) / b
    let numerator = a * wad + half_b;
    numerator / b
}

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/logic/GenericLogic.sol#L249
async fn get_user_balance_in_base_currency(
    provider: Arc<RootProvider<PubSubFrontend>>,
    reserve: Address,
    a_token_address: Address,
    user_address: Address,
    asset_price: U256,
    asset_unit: U256,
) -> U256 {
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

/// https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/libraries/logic/GenericLogic.sol#L219
async fn get_user_debt_in_base_currency(
    provider: Arc<RootProvider<PubSubFrontend>>,
    reserve: Address,
    variable_debt_token_address: Address,
    user_address: Address,
    asset_price: U256,
    asset_unit: U256,
) -> U256 {
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

/// This is the equivalent of _calculateUserAccountData() in LiquidationLogic.sol
/// https://github.com/aave-dao/aave-v3-origin/blob/bb6ea42947f349fe8182a0ea30c5a7883d1f9ed1/src/contracts/protocol/libraries/logic/GenericLogic.sol#L63
/// except for emode support. We don't do that here.
async fn calculate_user_account_data(
    price_cache: Arc<tokio::sync::Mutex<PriceCache>>,
    provider: Arc<RootProvider<PubSubFrontend>>,
    user_address: Address,
    reserves_list: Vec<Address>,
    reserves_data: Vec<AggregatedReserveData>,
) -> (U256, U256, U256) {
    // Capture required input arguments
    let mut total_collateral_in_base_currency = U256::ZERO;
    let mut total_debt_in_base_currency = U256::ZERO;
    let mut avg_liquidation_threshold = U256::ZERO;
    let mut health_factor = U256::ZERO;
    let user_account_data = (
        total_collateral_in_base_currency,
        total_debt_in_base_currency,
        health_factor,
    );
    let user_config = match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getUserConfiguration(user_address)
        .call()
        .await
    {
        Ok(user_config) => user_config._0,
        Err(e) => {
            eprintln!("Error trying to call getUserConfiguration: {}", e);
            return user_account_data;
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
                eprintln!("Error trying to get price for {}: {}", reserve_address, e);
                return user_account_data;
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
                    eprintln!("Error trying to call getIsVirtualAccActive: {}", e);
                    false
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
                            eprintln!("Error trying to call balanceOf for {}: {}", user_address, e);
                            U256::ZERO
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

    if total_debt_in_base_currency != U256::ZERO {
        let wad_numerator =
            percent_mul(total_collateral_in_base_currency, avg_liquidation_threshold);
        health_factor = wad_div(wad_numerator, total_debt_in_base_currency);
    } else {
        health_factor = U256::MAX;
    }

    // Return values
    (
        total_collateral_in_base_currency,
        total_debt_in_base_currency,
        health_factor,
    )
}

fn print_debt_collateral_title(
    total_combinations: usize,
    current_count: i32,
    borrowed_reserve: UserReserveData,
    supplied_reserve: UserReserveData,
    reserves_configuration: HashMap<Address, ReserveConfigurationEnhancedData>,
) {
    let borrowed_symbol = reserves_configuration
        .get(&borrowed_reserve.underlyingAsset)
        .unwrap()
        .symbol
        .clone();
    let supplied_symbol = reserves_configuration
        .get(&supplied_reserve.underlyingAsset)
        .unwrap()
        .symbol
        .clone();
    println!(
        "\t{}/{}) {} (debt) -> {} (collateral):",
        current_count, total_combinations, borrowed_symbol, supplied_symbol
    );
}

async fn get_asset_price(provider: Arc<RootProvider<PubSubFrontend>>, asset: Address) -> U256 {
    let aave_oracle = AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());
    match aave_oracle.getAssetPrice(asset).call().await {
        Ok(price_response) => price_response._0,
        Err(e) => {
            eprintln!("Error trying to call getAssetPrice: {}", e);
            U256::ZERO
        }
    }
}

/// https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L633
async fn calculate_available_collateral_to_liquidate(
    provider: Arc<RootProvider<PubSubFrontend>>,
    collateral_asset: Address,
    collateral_decimals: U256,
    // all original args for this function under this line
    collateral_asset_price: U256,
    collateral_asset_unit: U256,
    debt_asset_price: U256,
    debt_asset_unit: U256,
    mut debt_to_cover: U256,
    user_collateral_balance: U256,
    liquidation_bonus: U256,
) -> (U256, U256, U256, U256, U256) {
    let mut collateral_amount = U256::ZERO;
    let mut debt_amount_needed = U256::ZERO;
    let mut liquidation_protocol_fee = U256::ZERO;
    let mut collateral_to_liquidate_in_base_currency = U256::ZERO;

    let protocol =
        AaveProtocolDataProvider::new(AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let liquidation_protocol_fee_percentage = match protocol
        .getLiquidationProtocolFee(collateral_asset)
        .call()
        .await
    {
        Ok(response) => response._0,
        Err(e) => {
            eprintln!("Error trying to call collateralAToken.balanceOf(): {}", e);
            U256::ZERO
        }
    };
    let base_collateral = (debt_asset_price * debt_to_cover * collateral_asset_unit)
        / (collateral_asset_price * debt_asset_unit);
    let max_collateral_to_liquidate = percent_mul(base_collateral, liquidation_bonus);

    if max_collateral_to_liquidate > user_collateral_balance {
        collateral_amount = user_collateral_balance;
        debt_amount_needed = (collateral_asset_price * collateral_amount * debt_asset_unit)
            / percent_div(
                debt_asset_price * collateral_asset_unit,
                liquidation_bonus,
            )
    } else {
        collateral_amount = max_collateral_to_liquidate;
        debt_amount_needed = debt_to_cover;
    }
    println!(
        "\t\tv3.3 max collateral to liquidate: {}",
        max_collateral_to_liquidate
    );

    collateral_to_liquidate_in_base_currency =
        (collateral_amount * collateral_asset_price) / collateral_asset_unit;
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

    // TODO(Hernan): make gas and swap calculations more sophisticated
    let gas_used_estimation = U256::from(1000000);
    let gas_price_in_gwei = match provider.get_gas_price().await {
        Ok(price) => U256::from(price) / U256::from(1e3),
        _ => U256::MAX,
    };
    let execution_gas_cost = (gas_used_estimation * gas_price_in_gwei) / U256::from(1000000);
    // this assumes we will swap in 1% fee pools (could be more sophisticated)
    // uniswap v3 fees are represented as hundredths of basis points: 1% == 100; 0,3% == 30; 0,05% == 5; 0,01% == 1
    let swap_loss_factor = U256::from(100);
    let swap_total_cost = percent_mul(collateral_amount, swap_loss_factor);
    let net_profit = base_profit - execution_gas_cost - swap_total_cost;
    println!("\t\tv3.3 profit calculation:");
    println!(
        "\t\t\tbase profit = abs(collateral amount - debt in collateral units) = abs({} - {}) = {} ($ {})",
        collateral_amount,
        debt_in_collateral_units,
        base_profit,
        format_units(
            base_profit * collateral_asset_price,
            8 + u8::try_from(collateral_decimals).unwrap()
        )
        .unwrap()
    );
    println!(
        "\t\t\tdebt in collateral units: {}",
        debt_in_collateral_units
    );
    println!("\t\t\texecution gas cost: {}", execution_gas_cost);
    println!("\t\t\tswap total cost: {}", swap_total_cost);
    println!("\t\t\tnet profit = col amount - debt in col units - execution cost - swap cost = {} ($ {})", net_profit, format_units(net_profit * collateral_asset_price, 8 + u8::try_from(collateral_decimals).unwrap()).unwrap());

    (
        collateral_amount,
        debt_amount_needed,
        liquidation_protocol_fee,
        collateral_to_liquidate_in_base_currency,
        net_profit,
    )
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
                println!("Error fetching pool: {}", e);
                Address::ZERO
            }
        };
        let pool = UniswapV3Pool::new(pool_address, provider.clone());
        let in_range_liquidity = match pool.liquidity().call().await {
            Ok(response) => response._0,
            Err(e) => {
                println!("Error fetching pool liquidity: {}", e);
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
async fn calculate_best_swap_fees(
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
        println!("\t\tCollateral is WETH, no need to swap this leg");
    }

    // Get WETH -> debt pools
    let debt_pools = get_uniswap_v3_pools(&provider, WETH, debt_asset).await;
    println!("\t\tFound {} WETH/debt pools:", debt_pools.len());
    for (addr, fee, in_range_liquidity) in &debt_pools {
        println!(
            "\t\t- Liquidity {} with {}hbps fee at pool {}",
            in_range_liquidity, fee, addr
        );
    }
    best_fees.1 = debt_pools[0].1;

    // TODO: Calculate best fees based on liquidity and price impact

    best_fees
}

/// Iterates over all available (collateral, debt) pairs and returns the best one
/// The biggest difference between this one and the one from calculations.rs is the
/// way they deal with prices (this one, from the fork itself, and the one from calculations,
/// from the price cache)
async fn get_best_liquidation_opportunity(
    assets_borrowed: Vec<UserReserveData>,
    assets_supplied: Vec<UserReserveData>,
    reserves_configuration: HashMap<Address, ReserveConfigurationEnhancedData>,
    reserves_data: Vec<AggregatedReserveData>,
    provider: Arc<RootProvider<PubSubFrontend>>,
    user_address: Address,
    health_factor_v33: U256,
    total_debt_in_base_currency: U256,
) -> Option<BestPair> {
    // Essentially, inspect executeLiquidationCall internals
    // for every collateral/debt pair possible
    let mut best_pair: Option<BestPair> = None;
    let total_combinations = assets_borrowed.len() * assets_supplied.len();
    let mut current_count = 1;
    for borrowed_reserve in assets_borrowed
        .clone()
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::ZERO)
    {
        for supplied_reserve in assets_supplied
            .clone()
            .iter()
            .filter(|r| r.scaledATokenBalance > U256::ZERO && r.usageAsCollateralEnabledOnUser)
        {
            // 2/5) WETH (debt) -> WBTC (collateral):
            print_debt_collateral_title(
                total_combinations,
                current_count,
                borrowed_reserve.clone(),
                supplied_reserve.clone(),
                reserves_configuration.clone(),
            );

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L234-L238
            let (
                collateral_reserve,
                user_collateral_balance,
                debt_reserve,
                user_reserve_debt
            ) = match calculate_user_balances(
                reserves_data.clone(),
                supplied_reserve,
                borrowed_reserve,
                provider.clone(),
                user_address,
            ).await {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Error calculating user balances: {}", e);
                    continue;
                }
            };
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L234-L238
            println!(
                "\t\tv3.3 (user_collateral_balance, user_reserve_debt): {} / {}",
                user_collateral_balance, user_reserve_debt
            );

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L252-L276
            // TODO(Hernan): you should at least visually check if liquidationBonus is returning what you're expecting, since
            // the solidity implementation uses bit masking to get the value.
            let liquidation_bonus = collateral_reserve.reserveLiquidationBonus;
            let collateral_asset_price =
                get_asset_price(provider.clone(), supplied_reserve.underlyingAsset).await;
            let debt_asset_price =
                get_asset_price(provider.clone(), borrowed_reserve.underlyingAsset).await;
            let collateral_asset_unit = U256::from(10).pow(collateral_reserve.decimals);
            let debt_asset_unit = U256::from(10).pow(debt_reserve.decimals);
            let user_reserve_debt_in_base_currency =
                user_reserve_debt * debt_asset_price / debt_asset_unit;
            let user_reserve_collateral_in_base_currency =
                user_collateral_balance * collateral_asset_price / collateral_asset_unit;
            println!("\t\tv3.3 liquidation_bonus: {}", liquidation_bonus);
            println!(
                "\t\tv3.3 collateral: (price, unit, in_base_currency): ({}, {}, {})",
                collateral_asset_price,
                collateral_asset_unit,
                user_reserve_collateral_in_base_currency
            );
            println!(
                "\t\tv3.3 debt: (price, unit, in_base_currency): ({}, {}, {})",
                debt_asset_price, debt_asset_unit, user_reserve_debt_in_base_currency
            );
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
            println!(
                "\t\tv3.3 actual debt to liquidate: {}",
                actual_debt_to_liquidate
            );
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L278-L302

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L309
            let (
                actual_collateral_to_liquidate,
                actual_debt_to_liquidate,
                liquidation_protocol_fee_amount,
                collateral_to_liquidate_in_base_currency,
                net_profit,
            ) = calculate_available_collateral_to_liquidate(
                provider.clone(),
                collateral_reserve.underlyingAsset,
                collateral_reserve.decimals,
                collateral_asset_price,
                collateral_asset_unit,
                debt_asset_price,
                debt_asset_unit,
                actual_debt_to_liquidate,
                user_collateral_balance,
                liquidation_bonus,
            )
            .await;
            println!("\t\tv3.3 actual collateral to liquidate, actual debt to liquidate, fee amount, collateral to liquidate in base currency = {} / {} / {} / {}", actual_collateral_to_liquidate, actual_debt_to_liquidate, liquidation_protocol_fee_amount, collateral_to_liquidate_in_base_currency);
            println!(""); // space before next pair
                          // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L309

            // begin section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L320-L344
            // TODO(Hernan): do we need to make sure this doesn't bite us in the ass?
            // end section https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L320-L344

            if net_profit > best_pair.as_ref().map_or(U256::ZERO, |p| p.net_profit) {
                best_pair = Some(BestPair {
                    collateral_asset: supplied_reserve.underlyingAsset,
                    debt_asset: borrowed_reserve.underlyingAsset,
                    net_profit,
                    actual_collateral_to_liquidate,
                    actual_debt_to_liquidate,
                    liquidation_protocol_fee_amount,
                });
            }

            current_count += 1;
        }
    }
    best_pair
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() <= 2 {
        eprintln!("Usage: {} <address> [path_to_ipc]", args[0]);
        std::process::exit(1);
    }

    let ipc_path = args.get(2).map_or("/tmp/reth.ipc", |path| path.as_str());

    let user_address: Address = args[1].parse().expect("Invalid address format");

    // Setup provider
    let ipc = IpcConnect::new(ipc_path.to_string());
    let provider = ProviderBuilder::new().on_ipc(ipc).await.unwrap();
    let provider = Arc::new(provider);

    let block_number = provider.get_block_number().await.unwrap_or_default();
    println!(
        "Received address: {:?} at block {} (IPC: {})",
        user_address, block_number, ipc_path,
    );

    // Get user reserves data
    let user_reserves_data = get_user_reserves_data(provider.clone(), user_address).await;

    // Create reserve configuration struct
    let reserves_configuration =
        generate_reserve_details_by_asset(provider.clone()).await.unwrap();
    let assets_borrowed = user_reserves_data
        .iter()
        .filter(|reserve| reserve.scaledVariableDebt > U256::ZERO)
        .cloned()
        .collect::<Vec<UserReserveData>>();
    let assets_supplied = user_reserves_data
        .iter()
        .filter(|reserve| {
            reserve.usageAsCollateralEnabledOnUser && reserve.scaledATokenBalance > U256::ZERO
        })
        .cloned()
        .collect::<Vec<UserReserveData>>();

    // Get reserves data (not to be confused with UserReserveData) and reserves_list, which are aligned
    // (read `calculate_user_account_data()` comments about what this means)
    // `reserves_data` is Vec<AggregatedReserveData> and holds information about reserves in general,
    // while `user_reserves_data` holds information about a particular user's reserves
    // they're not the same
    let reserves_list = get_reserves_list(provider.clone()).await.unwrap();
    let reserves_data = get_reserves_data(provider.clone()).await.unwrap();

    // max_traces is 0 because we only use the price fetching feature for compatibility with
    // `calculate_user_account_data`, not the actual cache.
    let price_cache = Arc::new(Mutex::new(PriceCache::new(0)));

    // Calculate user account data
    let (total_collateral_in_base_currency, total_debt_in_base_currency, health_factor_v33) =
        calculate_user_account_data(
            price_cache.clone(),
            provider.clone(),
            user_address,
            reserves_list.clone(),
            reserves_data.clone(),
        )
        .await;
    println!("\n### User HF (value calculated with v3.3) ###");
    println!(
        "\t Total collateral (in base units): {}",
        total_collateral_in_base_currency
    );
    println!(
        "\t Total debt (in base units): {}",
        total_debt_in_base_currency
    );
    println!(
        "\t Health Factor: {}",
        format_units(health_factor_v33, "eth").unwrap()
    );

    let user_health_factor = get_user_health_factor(provider.clone(), user_address).await;
    println!("\n### User HF (value GET'd) ###");
    println!("\t {}", format_units(user_health_factor, "eth").unwrap());

    // Print user reserves data
    println!("\n### User DEBT (from getUserReservesData() array) ###");
    for reserve in assets_borrowed.clone() {
        let symbol = reserves_configuration
            .get(&reserve.underlyingAsset)
            .unwrap()
            .symbol
            .clone();
        let decimals = reserves_configuration
            .get(&reserve.underlyingAsset)
            .unwrap()
            .data
            .decimals
            .to::<u8>();
        println!(
            "\t{} - {} ({:?} units)",
            symbol,
            reserve.scaledVariableDebt,
            format_units(reserve.scaledVariableDebt, decimals).unwrap(),
        );
    }
    println!("\n### User COLLATERAL (from getUserReservesData() array) ###");
    for reserve in assets_supplied.clone() {
        let symbol = reserves_configuration
            .get(&reserve.underlyingAsset)
            .unwrap()
            .symbol
            .clone();
        let decimals = reserves_configuration
            .get(&reserve.underlyingAsset)
            .unwrap()
            .data
            .decimals
            .to::<u8>();
        println!(
            "\t{} - {} ({:?} units)",
            symbol,
            reserve.scaledATokenBalance,
            format_units(reserve.scaledATokenBalance, decimals).unwrap(),
        )
    }

    // Print number of possible combinations
    println!("\n### Liquidation path analysis ###");

    println!("\n### Most profitable liquidation opportunity ###");
    if let Some(best) = get_best_liquidation_opportunity(
        assets_borrowed,
        assets_supplied,
        reserves_configuration.clone(),
        reserves_data.clone(),
        provider.clone(),
        user_address,
        health_factor_v33,
        total_debt_in_base_currency,
    ).await {
        let debt_symbol = reserves_configuration
            .get(&best.debt_asset)
            .unwrap()
            .symbol
            .clone();
        let collateral_symbol = reserves_configuration
            .get(&best.collateral_asset)
            .unwrap()
            .symbol
            .clone();

        println!("\tliquidationCall(");
        println!(
            "\t\tcollateralAsset = {}, # {}",
            best.collateral_asset, collateral_symbol
        );
        println!("\t\tdebtAsset = {}, # {}", best.debt_asset, debt_symbol,);
        println!("\t\tuser = {},", user_address);
        println!("\t\tdebtToCover = {},", best.actual_debt_to_liquidate);
        println!("\t\treceiveAToken = false,");
        println!("\t)");

        let (collateral_to_weth_fee, weth_to_debt_fee) =
            calculate_best_swap_fees(provider.clone(), best.collateral_asset, best.debt_asset)
                .await;

        println!("\n### Foxdie ***TEST*** inputs ###");
        println!("export DEBT_SYMBOL={} && \\", debt_symbol);
        println!("export {}={} && \\", debt_symbol, best.debt_asset);
        println!("export COLLATERAL_SYMBOL={} && \\", collateral_symbol);
        println!(
            "export {}={} && \\",
            collateral_symbol, best.collateral_asset
        );
        println!("export USER_TO_LIQUIDATE={} && \\", user_address);
        println!("export DEBT_AMOUNT={} && \\", best.actual_debt_to_liquidate);
        println!(
            "export PRICE_UPDATER={} && \\",
            std::env::var("PRICE_UPDATE_FROM")
                .unwrap_or_else(|_| "Couldn't read PRICE_UPDATE_FROM from env".to_string())
        );
        println!(
            "export PRICE_UPDATE_TX_HASH={} && \\",
            std::env::var("PRICE_UPDATE_TX")
                .unwrap_or_else(|_| "Couldn't read PRICE_UPDATE_TX from env".to_string())
        );
        println!("export PRICE_UPDATE_BLOCK={} && \\", block_number - 1); // One less because forge will also replay the price update tx
        println!(
            "export COLLATERAL_TO_WETH_FEE={} && \\",
            collateral_to_weth_fee.to_string()
        );
        println!(
            "export WETH_TO_DEBT_FEE={} && \\",
            weth_to_debt_fee.to_string()
        );
        println!("export BUILDER_BRIBE={} && \\", "0"); // TODO
        println!("forge test --match-test testLiquidation -vvvvv");
        println!("\n");
    }
}
