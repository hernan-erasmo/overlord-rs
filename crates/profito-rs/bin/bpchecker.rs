use alloy::{
    primitives::{address, utils::format_units, Address, U256},
    providers::{IpcConnect, Provider, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol_types::sol,
};
use profito_rs::{
    calculations::{get_best_fee_tier_for_swap, percent_div, percent_mul},
    constants::{
        AAVE_ORACLE_ADDRESS, AAVE_V3_POOL_ADDRESS, AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS,
        AAVE_V3_PROVIDER_ADDRESS, AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
    },
    sol_bindings::{
        pool::AaveV3Pool,
        AaveOracle, AaveProtocolDataProvider, AaveUIPoolDataProvider, IAToken,
        IUiPoolDataProviderV3::{AggregatedReserveData, UserReserveData},
        ERC20,
    },
    utils::{ReserveConfigurationData, ReserveConfigurationEnhancedData},
};
use std::{collections::HashMap, env};

#[derive(Debug)]
struct BestPair {
    collateral_asset: Address,
    debt_asset: Address,
    net_profit: U256,
    actual_collateral_to_liquidate: U256,
    actual_debt_to_liquidate: U256,
    liquidation_protocol_fee_amount: U256,
}

async fn generate_reserve_details_by_asset(
    provider: RootProvider<PubSubFrontend>,
    reserves: Vec<UserReserveData>,
) -> ReserveConfigurationData {
    let mut symbols_by_address = HashMap::new();
    symbols_by_address.insert(
        address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
        "WETH".to_string(),
    );
    symbols_by_address.insert(
        address!("7f39c581f595b53c5cb19bd0b3f8da6c935e2ca0"),
        "wstETH".to_string(),
    );
    symbols_by_address.insert(
        address!("2260fac5e5542a773aa44fbcfedf7c193bc2c599"),
        "WBTC".to_string(),
    );
    symbols_by_address.insert(
        address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        "USDC".to_string(),
    );
    symbols_by_address.insert(
        address!("6b175474e89094c44da98b954eedeac495271d0f"),
        "DAI".to_string(),
    );
    symbols_by_address.insert(
        address!("514910771af9ca656af840dff83e8264ecf986ca"),
        "LINK".to_string(),
    );
    symbols_by_address.insert(
        address!("7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9"),
        "AAVE".to_string(),
    );
    symbols_by_address.insert(
        address!("be9895146f7af43049ca1c1ae358b0541ea49704"),
        "cbETH".to_string(),
    );
    symbols_by_address.insert(
        address!("dac17f958d2ee523a2206206994597c13d831ec7"),
        "USDT".to_string(),
    );
    symbols_by_address.insert(
        address!("ae78736cd615f374d3085123a210448e74fc6393"),
        "rETH".to_string(),
    );
    symbols_by_address.insert(
        address!("5f98805a4e8be255a32880fdec7f6728c6568ba0"),
        "LUSD".to_string(),
    );
    symbols_by_address.insert(
        address!("d533a949740bb3306d119cc777fa900ba034cd52"),
        "CRV".to_string(),
    );
    symbols_by_address.insert(
        address!("9f8f72aa9304c8b593d555f12ef6589cc3a579a2"),
        "MKR".to_string(),
    );
    symbols_by_address.insert(
        address!("c011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f"),
        "SNX".to_string(),
    );
    symbols_by_address.insert(
        address!("ba100000625a3754423978a60c9317c58a424e3d"),
        "BAL".to_string(),
    );
    symbols_by_address.insert(
        address!("1f9840a85d5af5bf1d1762f925bdaddc4201f984"),
        "UNI".to_string(),
    );
    symbols_by_address.insert(
        address!("5a98fcbea516cf06857215779fd812ca3bef1b32"),
        "LDO".to_string(),
    );
    symbols_by_address.insert(
        address!("c18360217d8f7ab5e7c516566761ea12ce7f9d72"),
        "ENS".to_string(),
    );
    symbols_by_address.insert(
        address!("111111111117dc0aa78b770fa6a738034120c302"),
        "1INCH".to_string(),
    );
    symbols_by_address.insert(
        address!("853d955acef822db058eb8505911ed77f175b99e"),
        "FRAX".to_string(),
    );
    symbols_by_address.insert(
        address!("40d16fc0246ad3160ccc09b8d0d3a2cd28ae6c2f"),
        "GHO".to_string(),
    );
    symbols_by_address.insert(
        address!("d33526068d116ce69f19a9ee46f0bd304f21a51f"),
        "RPL".to_string(),
    );
    symbols_by_address.insert(
        address!("83f20f44975d03b1b09e64809b757c47f942beea"),
        "sDAI".to_string(),
    );
    symbols_by_address.insert(
        address!("af5191b0de278c7286d6c7cc6ab6bb8a73ba2cd6"),
        "STG".to_string(),
    );
    symbols_by_address.insert(
        address!("defa4e8a7bcba345f687a2f1456f5edd9ce97202"),
        "KNC".to_string(),
    );
    symbols_by_address.insert(
        address!("3432b6a60d23ca0dfca7761b7ab56459d9c964d0"),
        "FXS".to_string(),
    );
    symbols_by_address.insert(
        address!("f939e0a03fb07f59a73314e73794be0e57ac1b4e"),
        "crvUSD".to_string(),
    );
    symbols_by_address.insert(
        address!("6c3ea9036406852006290770bedfcaba0e23a0e8"),
        "PYUSD".to_string(),
    );
    symbols_by_address.insert(
        address!("cd5fe23c85820f7b72d0926fc9b05b43e359b7ee"),
        "weETH".to_string(),
    );
    symbols_by_address.insert(
        address!("f1c9acdc66974dfb6decb12aa385b9cd01190e38"),
        "osETH".to_string(),
    );
    symbols_by_address.insert(
        address!("4c9edd5852cd905f086c759e8383e09bff1e68b3"),
        "USDe".to_string(),
    );
    symbols_by_address.insert(
        address!("a35b1b31ce002fbf2058d22f30f95d405200a15b"),
        "ETHx".to_string(),
    );
    symbols_by_address.insert(
        address!("9d39a5de30e57443bff2a8307a4256c8797a3497"),
        "sUSDe".to_string(),
    );
    symbols_by_address.insert(
        address!("18084fba666a33d37592fa2633fd49a74dd93a88"),
        "tBTC".to_string(),
    );
    symbols_by_address.insert(
        address!("cbb7c0000ab88b473b1f5afd9ef808440eed33bf"),
        "cbBTC".to_string(),
    );
    symbols_by_address.insert(
        address!("dc035d45d973e3ec169d2276ddab16f1e407384f"),
        "USDS".to_string(),
    );
    symbols_by_address.insert(
        address!("a1290d69c65a6fe4df752f95823fae25cb99e5a7"),
        "rsETH".to_string(),
    );
    symbols_by_address.insert(
        address!("8236a87084f8b84306f72007f36f2618a5634494"),
        "LBTC".to_string(),
    );

    let mut configuration_data: ReserveConfigurationData = HashMap::new();
    let aave_config =
        AaveProtocolDataProvider::new(AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let unknown_asset = String::from("unknown_asset");
    for reserve in reserves {
        let symbol = symbols_by_address
            .get(&reserve.underlyingAsset)
            .unwrap_or(&unknown_asset);
        let data = match aave_config
            .getReserveConfigurationData(reserve.underlyingAsset)
            .call()
            .await
        {
            Ok(reserve_config) => Some(reserve_config),
            Err(e) => {
                eprintln!(
                    "Failed to get reserve configuration data for asset {}: {}",
                    reserve.underlyingAsset, e
                );
                None
            }
        };
        let liquidation_fee = match aave_config
            .getLiquidationProtocolFee(reserve.underlyingAsset)
            .call()
            .await
        {
            Ok(fee_response) => Some(fee_response._0),
            Err(e) => {
                eprintln!(
                    "Failed to get reserve liquidation fee for asset {}: {}",
                    reserve.underlyingAsset, e
                );
                None
            }
        };
        if let (Some(data), Some(liquidation_fee)) = (data, liquidation_fee) {
            configuration_data.insert(
                reserve.underlyingAsset,
                ReserveConfigurationEnhancedData {
                    symbol: symbol.clone(),
                    data,
                    liquidation_fee,
                },
            );
        }
    }
    configuration_data
}

/// Get's the list of user reserves, but only returns those that the user has at least some debt or collateral and,
/// for the later, the ones that are allowed to be used as collateral
async fn get_user_reserves_data(
    provider: RootProvider<PubSubFrontend>,
    user_address: Address,
) -> Vec<UserReserveData> {
    let ui_data =
        AaveUIPoolDataProvider::new(AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let user_reserves_data = match ui_data
        .getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, user_address)
        .call()
        .await
    {
        Ok(user_reserves) => user_reserves._0,
        Err(e) => {
            eprintln!("Error trying to call AaveUIPoolDataProvider: {}", e);
            std::process::exit(1);
        }
    };
    user_reserves_data
        .iter()
        .filter(|reserve| {
            reserve.scaledVariableDebt > U256::ZERO
                || (reserve.scaledATokenBalance > U256::ZERO
                    && reserve.usageAsCollateralEnabledOnUser)
        })
        .cloned()
        .collect()
}

async fn get_user_health_factor(provider: RootProvider<PubSubFrontend>, user: Address) -> U256 {
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
    provider: RootProvider<PubSubFrontend>,
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

async fn calculate_pair_profitability(
    provider: RootProvider<PubSubFrontend>,
    borrowed_reserve: UserReserveData,
    supplied_reserve: UserReserveData,
    reserves_configuration: HashMap<Address, ReserveConfigurationEnhancedData>,
    liquidation_close_factor: U256,
    mut actual_debt_to_liquidate: U256,
) -> (U256, U256, U256, U256) {
    let debt_config = reserves_configuration
        .get(&borrowed_reserve.underlyingAsset)
        .unwrap();
    let collateral_config = reserves_configuration
        .get(&supplied_reserve.underlyingAsset)
        .unwrap();
    let liquidation_protocol_fee_amount = U256::ZERO;
    let aave_oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, RootProvider<PubSubFrontend>> =
        AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());
    let debt_asset_price = match aave_oracle
        .getAssetPrice(borrowed_reserve.underlyingAsset)
        .call()
        .await
    {
        Ok(price_response) => price_response._0,
        Err(e) => {
            eprintln!("Error trying to call getAssetPrice: {}", e);
            std::process::exit(1);
        }
    };
    let collateral_asset_price = match aave_oracle
        .getAssetPrice(supplied_reserve.underlyingAsset)
        .call()
        .await
    {
        Ok(price_response) => price_response._0,
        Err(e) => {
            eprintln!("Error trying to call getAssetPrice: {}", e);
            std::process::exit(1);
        }
    };
    println!(
        "\t\tprice per debt unit (debt_asset_price) = {} ($ {})",
        debt_asset_price,
        format_units(debt_asset_price, 8).unwrap()
    );
    println!(
        "\t\tprice per collateral unit (collateral_asset_price) = {} ($ {})",
        collateral_asset_price,
        format_units(collateral_asset_price, 8).unwrap()
    );

    let debt_asset_decimals = debt_config.data.decimals.to::<u8>();
    let collateral_asset_decimals = collateral_config.data.decimals.to::<u8>();

    let debt_asset_unit = U256::from(10).pow(U256::from(debt_asset_decimals));
    let collateral_asset_unit = U256::from(10).pow(U256::from(collateral_asset_decimals));

    let base_collateral = (debt_asset_price * actual_debt_to_liquidate * collateral_asset_unit)
        / (collateral_asset_price * debt_asset_unit);

    println!(
        "\t\tactual debt to liquidate (debt x liquidation factor) ({} x {}) = {} ($ {})",
        borrowed_reserve.scaledVariableDebt,
        format_units(liquidation_close_factor, 4).unwrap(),
        actual_debt_to_liquidate,
        format_units(
            percent_mul(
                borrowed_reserve.scaledVariableDebt,
                liquidation_close_factor
            ) * debt_asset_price,
            debt_asset_decimals + 8
        )
        .unwrap(),
    );
    println!(
        "\t\tbase collateral = {} ({} units) ($ {})",
        base_collateral,
        format_units(base_collateral, collateral_asset_decimals).unwrap(),
        format_units(
            base_collateral * collateral_asset_price,
            collateral_asset_decimals + 8
        )
        .unwrap(),
    );

    let max_collateral_to_liquidate =
        percent_mul(base_collateral, collateral_config.data.liquidationBonus);
    println!(
        "\t\tmax collateral to liquidate ({}% of base collateral) = {} ($ {})",
        format_units(collateral_config.data.liquidationBonus, 2).unwrap(),
        max_collateral_to_liquidate,
        format_units(
            max_collateral_to_liquidate * collateral_asset_price,
            collateral_asset_decimals + 8
        )
        .unwrap(),
    );

    // Just the same as LiquidationLogic does, we need to make sure the user has enough
    // collateral to cover max_collateral_to_liquidate. If not, then we need to adjust
    // the amount of debt to liquidate accordingly.
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

    if max_collateral_to_liquidate > supplied_reserve.scaledATokenBalance {
        println!("\t\tNOT ENOUGH COLLATERAL TO DEDUCT MAX");
        println!(
            "\t\t\tnew debt to liquidate = ({}) ({} units) ($ {})",
            actual_debt_to_liquidate,
            format_units(actual_debt_to_liquidate, debt_asset_decimals).unwrap(),
            format_units(
                actual_debt_to_liquidate * debt_asset_price,
                debt_asset_decimals + 8
            )
            .unwrap(),
        );
        println!(
            "\t\t\tnew collateral to liquidate = ({}) ({} units) ($ {})",
            collateral_amount,
            format_units(collateral_amount, collateral_asset_decimals).unwrap(),
            format_units(
                collateral_amount * collateral_asset_price,
                collateral_asset_decimals + 8
            )
            .unwrap(),
        );
    }

    // At this point, all sanity checks on debt and collateral amounts to liquidate have passed
    // `collateral_amount` contains the valid maximum amount to liquidate on this pair, and
    // `amount_debt_to_liquidate` contains the valid maximum amount of debt to repay.

    let mut bonus_collateral = U256::ZERO;
    let mut liquidation_fee = U256::ZERO;
    if collateral_config.liquidation_fee != U256::ZERO {
        bonus_collateral = collateral_amount
            - percent_div(collateral_amount, collateral_config.data.liquidationBonus);
        liquidation_fee = percent_mul(bonus_collateral, collateral_config.liquidation_fee);
    }
    println!(
        "\t\tbonus collateral (max - base) = {} ($ {})",
        bonus_collateral,
        format_units(
            bonus_collateral * collateral_asset_price,
            collateral_asset_decimals + 8
        )
        .unwrap(),
    );
    println!(
        "\t\tliquidation fee ({}% of bonus collateral) = {} ($ {})",
        format_units(collateral_config.liquidation_fee, 2).unwrap(),
        liquidation_fee,
        format_units(
            liquidation_fee * collateral_asset_price,
            collateral_asset_decimals + 8
        )
        .unwrap(),
    );

    let actual_collateral_to_liquidate = collateral_amount - liquidation_fee;
    println!(
        "\t\tcollateral to liquidate = {} ($ {})",
        actual_collateral_to_liquidate,
        format_units(
            actual_collateral_to_liquidate * collateral_asset_price,
            collateral_asset_decimals + 8
        )
        .unwrap()
    );

    // These aren't relevant to AAVE, that's why you won't find anything on them in Aave code
    // In order to calculate net profit, everthing must be denominated in collateral units
    // otherwise it will return garbage
    let debt_in_collateral_units =
        (actual_debt_to_liquidate * debt_asset_price * collateral_asset_unit)
            / (collateral_asset_price * debt_asset_unit);
    println!("\t\tdebt in collateral units:");
    println!(
        "\t\t\t{} x {} x {}",
        actual_debt_to_liquidate, debt_asset_price, collateral_asset_unit
    );
    println!("\t\t\t-------------------------------------------------");
    println!("\t\t\t{} x {}", collateral_asset_price, debt_asset_unit);

    // THIS IS THE CORE OF THE CALCULATION, WHAT DECIDES WHETHER OR NOT WE MOVE ON WITH THE EXECUTION
    let gas_used_estimation = U256::from(1000000); // TODO(Hernan): good-enough this
    let gas_price_in_gwei = match provider.get_gas_price().await {
        Ok(price) => U256::from(price) / U256::from(1e3),
        Err(e) => U256::MAX,
    };
    let execution_gas_cost = (gas_used_estimation * gas_price_in_gwei) / U256::from(1000000);
    let swap_loss_factor = U256::from(10000); // this assumes we will swap in 1% fee pools (could be more sophisticated)
    let swap_total_cost = actual_collateral_to_liquidate
        - percent_div(actual_collateral_to_liquidate, swap_loss_factor);
    let net_profit = actual_collateral_to_liquidate - // This already has the liquidation fee deducted
        debt_in_collateral_units -
        execution_gas_cost -
        swap_total_cost;

    println!(
        "\t\tnet profit (collateral reward - debt repaid - swap cost - execution cost) ({} - {} - {} - {})\n\t\t\t{} ($ {})",
        actual_collateral_to_liquidate, // collateral units
        debt_in_collateral_units,
        swap_total_cost,
        execution_gas_cost,
        net_profit,
        format_units(net_profit * collateral_asset_price, collateral_asset_decimals + 8).unwrap(),
    );

    // This is the actual return tuple from _calculateAvailableCollateralToLiquidate()
    (
        net_profit,
        actual_collateral_to_liquidate,
        actual_debt_to_liquidate,
        liquidation_protocol_fee_amount,
    )
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
    provider: RootProvider<PubSubFrontend>,
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
    provider: RootProvider<PubSubFrontend>,
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
    provider: RootProvider<PubSubFrontend>,
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
        let asset_price = match AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone())
            .getAssetPrice(reserve_address)
            .call()
            .await
        {
            Ok(price_response) => price_response._0,
            Err(e) => {
                eprintln!("Error trying to call getAssetPrice: {}", e);
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

async fn get_reserves_list(provider: RootProvider<PubSubFrontend>) -> Vec<Address> {
    /*
       According to https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/protocol/pool/Pool.sol#L532
       the reserves list is ordered the same way as the _reserveList storage in the Pool contract.
    */
    match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getReservesList()
        .call()
        .await
    {
        Ok(reserves_list) => reserves_list._0,
        Err(e) => {
            eprintln!("Error trying to call getReservesList: {}", e);
            return Vec::new();
        }
    }
}

async fn get_reserves_data(provider: RootProvider<PubSubFrontend>) -> Vec<AggregatedReserveData> {
    /*
       According to https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/helpers/UiPoolDataProviderV3.sol#L45
       the reserves data is ordered the same way as the reserves list (it actually calls pool.getReservesList() and uses it as index)
    */
    match AaveUIPoolDataProvider::new(AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, provider.clone())
        .getReservesData(AAVE_V3_PROVIDER_ADDRESS)
        .call()
        .await
    {
        Ok(reserves_list) => reserves_list._0,
        Err(e) => {
            eprintln!("Error trying to call getReservesData: {}", e);
            return Vec::new();
        }
    }
}

async fn get_asset_price(provider: RootProvider<PubSubFrontend>, asset: Address) -> U256 {
    let aave_oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, RootProvider<PubSubFrontend>> =
        AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());
    match aave_oracle.getAssetPrice(asset).call().await {
        Ok(price_response) => price_response._0,
        Err(e) => {
            eprintln!("Error trying to call getAssetPrice: {}", e);
            U256::ZERO
        }
    }
}

fn calculate_actual_debt_to_liquidate(
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

/// https://github.com/aave-dao/aave-v3-origin/blob/e8f6699e58038cbe3aba982557ceb2b0dda303a0/src/contracts/protocol/libraries/logic/LiquidationLogic.sol#L633
async fn calculate_available_collateral_to_liquidate(
    provider: RootProvider<PubSubFrontend>,
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
                (debt_asset_price * collateral_asset_unit),
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
        Err(e) => U256::MAX,
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

    let block_number = provider.get_block_number().await.unwrap_or_default();
    println!(
        "Received address: {:?} at block {} (IPC: {})",
        user_address, block_number, ipc_path,
    );

    // Get user reserves data
    let user_reserves_data = get_user_reserves_data(provider.clone(), user_address).await;

    // Create reserve configuration struct
    let reserves_configuration =
        generate_reserve_details_by_asset(provider.clone(), user_reserves_data.clone()).await;
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
    let reserves_list = get_reserves_list(provider.clone()).await;
    let reserves_data = get_reserves_data(provider.clone()).await;

    // Calculate user account data
    let (total_collateral_in_base_currency, total_debt_in_base_currency, health_factor_v33) =
        calculate_user_account_data(
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

    // Start iterating over available pairs
    // essentially inspecting executeLiquidationCall internals
    // for every collateral/debt pair possible
    let mut best_pair: Option<BestPair> = None;
    let total_combinations = assets_borrowed.len() * assets_supplied.len();
    let mut current_count = 1;
    for borrowed_reserve in assets_borrowed
        .clone()
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::from(0))
    {
        for supplied_reserve in assets_supplied
            .clone()
            .iter()
            .filter(|r| r.scaledATokenBalance > U256::from(0) && r.usageAsCollateralEnabledOnUser)
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
                    Err(e) => {
                        eprintln!("Error trying to call collateralAToken.balanceOf(): {}", e);
                        U256::ZERO
                    }
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
                Err(e) => {
                    eprintln!("Error trying to call debt_reserve_token.balanceOf: {}", e);
                    U256::ZERO
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

    println!("\n### Most profitable liquidation opportunity ###");
    if let Some(best) = best_pair {
        println!("\tliquidationCall(");
        println!(
            "\t\tcollateralAsset = {}, # {}",
            best.collateral_asset,
            reserves_configuration
                .get(&best.collateral_asset)
                .unwrap()
                .symbol
        );
        println!(
            "\t\tdebtAsset = {}, # {}",
            best.debt_asset,
            reserves_configuration.get(&best.debt_asset).unwrap().symbol
        );
        println!("\t\tuser = {},", user_address);
        println!("\t\tdebtToCover = {},", best.actual_debt_to_liquidate);
        println!("\t\treceiveAToken = false,");
        println!("\t)");
    }
}
