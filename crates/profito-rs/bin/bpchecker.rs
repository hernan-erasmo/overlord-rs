use alloy::{
    primitives::{address, Address, utils::format_units, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use std::{collections::HashMap, env};
use AaveProtocolDataProvider::getReserveConfigurationDataReturn;

mod pool {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        AaveV3Pool,
        "src/abis/aave_v3_pool.json"
    );
}

mod ui {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        AaveUIPoolDataProvider,
        "src/abis/aave_ui_pool_data_provider.json"
    );
}

use pool::AaveV3Pool;
use ui::AaveUIPoolDataProvider;
use ui::IUiPoolDataProviderV3::UserReserveData;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    AaveOracle,
    "src/abis/aave_v3_oracle.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    AaveProtocolDataProvider,
    "src/abis/aave_protocol_data_provider.json"
);

#[derive(Debug, Clone)]
struct ReserveConfigurationEnhancedData {
    symbol: String,
    data: getReserveConfigurationDataReturn,
    liquidation_fee: U256,
}

type ReserveConfigurationData = HashMap<Address, ReserveConfigurationEnhancedData>;

const AAVE_V3_POOL_ADDRESS: Address = address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2");
const AAVE_ORACLE_ADDRESS: Address = address!("0x54586bE62E3c3580375aE3723C145253060Ca0C2");
const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS: Address =
    address!("41393e5e337606dc3821075Af65AeE84D7688CBD");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address =
    address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");

async fn generate_reserve_details_by_asset(
    provider: RootProvider<PubSubFrontend>,
    reserves: Vec<ui::IUiPoolDataProviderV3::UserReserveData>,
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
    let aave_config = AaveProtocolDataProvider::new(
        AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS,
        provider.clone(),
    );
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
async fn get_user_reserves_data(provider: RootProvider<PubSubFrontend>, user_address: Address) -> Vec<UserReserveData> {
    let ui_data = AaveUIPoolDataProvider::new(
        AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
        provider.clone(),
    );
    let user_reserves_data: Vec<ui::IUiPoolDataProviderV3::UserReserveData>;

    match ui_data
        .getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, user_address)
        .call()
        .await {
            Ok(user_reserves) => {
                user_reserves_data = user_reserves._0;
            },
            Err(e) => {
                eprintln!("Error trying to call AaveUIPoolDataProvider: {}", e);
                std::process::exit(1);
            }
        }
    user_reserves_data.iter().filter(|reserve| {
        reserve.scaledVariableDebt > U256::ZERO || (reserve.scaledATokenBalance > U256::ZERO && reserve.usageAsCollateralEnabledOnUser)
    }).cloned().collect()
}

async fn get_user_health_factor(provider: RootProvider<PubSubFrontend>, user: Address) -> U256 {
    let pool = pool::AaveV3Pool::new(
        AAVE_V3_POOL_ADDRESS,
        provider.clone(),
    );
    let health_factor: U256;
    match pool
        .getUserAccountData(user)
        .call()
        .await {
        Ok(account_data) => {
            health_factor = account_data.healthFactor;
        },
        Err(e) => {
            eprintln!("Error trying to call getUserAccountData: {}", e);
            std::process::exit(1);
        }
    }
    health_factor
}

/// This mimics `percentMul` at
/// https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/math/PercentageMath.sol#L25
fn percent_mul(value: U256, percentage: U256) -> U256 {
    (value * percentage + U256::from(0.5e4)) / U256::from(1e4)
}

async fn calculate_pair_profitability(
    provider: RootProvider<PubSubFrontend>,
    borrowed_reserve: UserReserveData,
    supplied_reserve: UserReserveData,
    reserves_configuration: HashMap<Address, ReserveConfigurationEnhancedData>,
    liquidation_close_factor: U256,
    actual_debt_to_liquidate: U256,
) -> (U256, U256, U256) {
    let collateral_config = reserves_configuration.get(&supplied_reserve.underlyingAsset).unwrap();
    let debt_config = reserves_configuration.get(&supplied_reserve.underlyingAsset).unwrap();
    let actual_collateral_to_liquidate = U256::ZERO;
    let liquidation_protocol_fee_amount = U256::ZERO;
    let collateral_asset_price: U256;
    let debt_asset_price: U256;
    let aave_oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, RootProvider<PubSubFrontend>> = AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());
    match aave_oracle.getAssetPrice(borrowed_reserve.underlyingAsset).call().await {
        Ok(price_response) => {
            debt_asset_price = price_response._0;
        },
        Err(e) => {
            eprintln!("Error trying to call getAssetPrice: {}", e);
            std::process::exit(1);
        }
    };
    match aave_oracle.getAssetPrice(supplied_reserve.underlyingAsset).call().await {
        Ok(price_response) => {
            collateral_asset_price = price_response._0;
        },
        Err(e) => {
            eprintln!("Error trying to call getAssetPrice: {}", e);
            std::process::exit(1);
        }
    };
    println!("\t\tprice per debt unit (debt_asset_price) = {} ($ {})", debt_asset_price, format_units(debt_asset_price, 8).unwrap());
    println!("\t\tprice per collateral unit (collateral_asset_price) = {} ($ {})", collateral_asset_price, format_units(collateral_asset_price, 8).unwrap());

    let debt_asset_decimals = reserves_configuration.get(&borrowed_reserve.underlyingAsset).unwrap().data.decimals.to::<u8>();
    let collateral_asset_decimals = reserves_configuration.get(&supplied_reserve.underlyingAsset).unwrap().data.decimals.to::<u8>();

    let debt_asset_unit = U256::from(10).pow(U256::from(debt_asset_decimals));
    let collateral_asset_unit = U256::from(10).pow(U256::from(collateral_asset_decimals));

    let base_collateral =
    (debt_asset_price * actual_debt_to_liquidate * collateral_asset_unit)
        / (collateral_asset_price * debt_asset_unit);

    println!(
        "\t\tactual debt to liquidate (debt x liquidation factor) ({} x {}) = {} ($ {})",
        borrowed_reserve.scaledVariableDebt,
        format_units(liquidation_close_factor, 4).unwrap(),
        actual_debt_to_liquidate,
        format_units(borrowed_reserve.scaledVariableDebt * debt_asset_price, debt_asset_decimals + 8).unwrap(),
    );
    println!(
        "\t\tbase collateral = {} ({} units) ($ {})",
        base_collateral,
        format_units(base_collateral, collateral_asset_decimals).unwrap(),
        format_units(base_collateral * collateral_asset_price, collateral_asset_decimals + 8).unwrap(),
    );

    let max_collateral_to_liquidate =
    percent_mul(base_collateral, collateral_config.data.liquidationBonus);
    println!(
        "\t\tmax collateral to liquidate = {} ({}) ($ {})",
        max_collateral_to_liquidate,
        format_units(max_collateral_to_liquidate, collateral_asset_decimals).unwrap(),
        format_units(max_collateral_to_liquidate * collateral_asset_price, collateral_asset_decimals + 8).unwrap(),
    );

    (actual_collateral_to_liquidate, actual_debt_to_liquidate, liquidation_protocol_fee_amount)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() != 2 {
        eprintln!("Usage: {} <address>", args[0]);
        std::process::exit(1);
    }

    let user_address: Address = args[1].parse().expect("Invalid address format");

    println!("Received address: {:?}", user_address);

    // Setup provider
    let ipc = IpcConnect::new("/tmp/reth.ipc".to_string());
    let provider = ProviderBuilder::new().on_ipc(ipc).await.unwrap();

    // Get user reserves data
    let user_reserves_data = get_user_reserves_data(provider.clone(), user_address).await;

    // Create reserve configuration struct
    let reserves_configuration = generate_reserve_details_by_asset(provider.clone(), user_reserves_data.clone()).await;
    let assets_borrowed = user_reserves_data.iter().filter(|reserve| { reserve.scaledVariableDebt > U256::ZERO }).cloned().collect::<Vec<UserReserveData>>();
    let assets_supplied = user_reserves_data.iter().filter(|reserve| { reserve.usageAsCollateralEnabledOnUser && reserve.scaledATokenBalance > U256::ZERO }).cloned().collect::<Vec<UserReserveData>>();

    // Fetch user health factor
    let user_health_factor = get_user_health_factor(provider.clone(), user_address).await;
    println!("\n### User HF ###");
    println!("\t {}", format_units(user_health_factor, "eth").unwrap());

    let liquidation_close_factor: U256;
    if user_health_factor <= U256::from(0.95e18) {
        liquidation_close_factor = U256::from(1e4);
    } else {
        liquidation_close_factor = U256::from(0.5e4);
    };

    // Print user reserves data
    println!("\n### User DEBT ###");
    for reserve in assets_borrowed.clone() {
        let symbol = reserves_configuration.get(&reserve.underlyingAsset).unwrap().symbol.clone();
        let decimals = reserves_configuration.get(&reserve.underlyingAsset).unwrap().data.decimals.to::<u8>();
        println!(
            "\t{} - {} ({:?} units)",
            symbol,
            reserve.scaledVariableDebt,
            format_units(reserve.scaledVariableDebt, decimals).unwrap(),
        );
    }
    println!("\n### User COLLATERAL ###");
    for reserve in assets_supplied.clone() {
        let symbol = reserves_configuration.get(&reserve.underlyingAsset).unwrap().symbol.clone();
        let decimals = reserves_configuration.get(&reserve.underlyingAsset).unwrap().data.decimals.to::<u8>();
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
    let total_combinations = assets_borrowed.len() * assets_supplied.len();
    let mut current_count = 1;
    for borrowed_reserve in assets_borrowed.clone()
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::from(0))
    {
        for supplied_reserve in assets_supplied.clone()
            .iter()
            .filter(|r| r.scaledATokenBalance > U256::from(0) && r.usageAsCollateralEnabledOnUser)
        {
            let borrowed_symbol = reserves_configuration.get(&borrowed_reserve.underlyingAsset).unwrap().symbol.clone();
            let supplied_symbol = reserves_configuration.get(&supplied_reserve.underlyingAsset).unwrap().symbol.clone();
            println!("\t{}/{}) {} -> {}", current_count, total_combinations, borrowed_symbol, supplied_symbol);

            // This is what _calculateDebt() over at LiquidationLogic is supposed to do
            let actual_debt_to_liquidate =
            percent_mul(borrowed_reserve.scaledVariableDebt, liquidation_close_factor);

            // The following is what _calculateAvailableCollateralToLiquidate() over at LiquidationLogic is supposed to do
            let (
                actual_collateral_to_liquidate,
                actual_debt_to_liquidate,
                liquidation_protocol_fee_amount,
            ) = calculate_pair_profitability(
                provider.clone(),
                borrowed_reserve.clone(),
                supplied_reserve.clone(),
                reserves_configuration.clone(),
                liquidation_close_factor,
                actual_debt_to_liquidate,
            ).await;

            current_count += 1;
        }
    }
}
