use alloy::{
    primitives::{utils::format_units, Address, U256},
    providers::{IpcConnect, Provider, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
};
use ethers_core::{
    abi::{encode, Token, ParamType},
    types::{H256, transaction::eip2718::TypedTransaction, Eip1559TransactionRequest, U256 as ethersU256, H160},
    utils::{hex, keccak256},
};
use profito_rs::cache::PriceCache;
use profito_rs::{
    calculations::{
        BestPair,
        percent_div,
        percent_mul,
        calculate_actual_debt_to_liquidate,
        calculate_user_balances,
        get_reserves_list,
        get_reserves_data,
        calculate_user_account_data,
        calculate_best_swap_fees,
    },
    constants::{
        AAVE_ORACLE_ADDRESS, AAVE_V3_POOL_ADDRESS, AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS,
    },
    sol_bindings::{
        pool::AaveV3Pool,
        AaveOracle, AaveProtocolDataProvider,
        IUiPoolDataProviderV3::{AggregatedReserveData, UserReserveData},
    },
    utils::{ReserveConfigurationEnhancedData, generate_reserve_details_by_asset, get_user_reserves_data},
    mev_share_service::MevShareService,
};
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::Mutex;

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

/// ANY CHANGES TO THIS FUNCTION MUST ALSO BE REPLICATED IN THE
/// ONE DEFINED IN calculations.rs (SEE THAT FUNCTION DOCS FOR CONTEXT)
async fn calculate_available_collateral_to_liquidate(
    provider: Arc<RootProvider<PubSubFrontend>>,
    collateral_asset: Address,
    collateral_decimals: U256,
    // all original args for this function under this line
    collateral_asset_price: U256,
    collateral_asset_unit: U256,
    debt_asset_price: U256,
    debt_asset_unit: U256,
    debt_to_cover: U256,
    user_collateral_balance: U256,
    liquidation_bonus: U256,
) -> (U256, U256, U256, U256, U256) {
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
            eprintln!("Error trying to call collateralAToken.balanceOf(): {}", e);
            U256::ZERO
        }
    };
    let base_collateral = (debt_asset_price * debt_to_cover * collateral_asset_unit)
        / (collateral_asset_price * debt_asset_unit);
    let max_collateral_to_liquidate = percent_mul(base_collateral, liquidation_bonus);

    let mut collateral_amount: U256;
    let debt_amount_needed: U256;
    if max_collateral_to_liquidate > user_collateral_balance {
        collateral_amount = user_collateral_balance;
        debt_amount_needed = percent_div((collateral_asset_price * collateral_amount * debt_asset_unit) / (debt_asset_price * collateral_asset_unit), liquidation_bonus);
    } else {
        collateral_amount = max_collateral_to_liquidate;
        debt_amount_needed = debt_to_cover;
    }
    println!(
        "\t\tv3.3 max collateral to liquidate: {}",
        max_collateral_to_liquidate
    );

    let collateral_to_liquidate_in_base_currency: U256;
    let mut liquidation_protocol_fee = U256::ZERO;
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
    let total_cost = execution_gas_cost + swap_total_cost;
    let net_profit = if total_cost > base_profit {
        U256::MIN
    } else {
        base_profit - total_cost
    };
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
        (net_profit * collateral_asset_price) / collateral_asset_unit,
    )
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
                // net_profit comes denominated in base units,
                // comparable across different assets:
                //      (net_profit * collateral_asset_price) / collateral_asset_unit,
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
                    printable_net_profit: String::from(""), // we don't use printable_net_profit here
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
        eprintln!("Usage: {} <address> [path_to_ipc] [simulate_bundle]", args[0]);
        std::process::exit(1);
    }

    let ipc_path = args.get(2).map_or("/tmp/reth.ipc", |path| path.as_str());
    let simulate_bundle = args.get(3).is_some();

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
    let (
        total_collateral_in_base_currency,
        total_debt_in_base_currency,
        health_factor_v33
    ) = match calculate_user_account_data(
            price_cache.clone(),
            provider.clone(),
            user_address,
            reserves_list.clone(),
            reserves_data.clone(),
            None,
        ).await {
            Ok((collateral, debt, hf)) => (collateral, debt, hf),
            Err(e) => {
                eprintln!("Error calculating user account data: {}", e);
                std::process::exit(1);
            }
        };
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
        let price_update_tx_hash = std::env::var("PRICE_UPDATE_TX")
            .map(|hash| hash.parse::<H256>().unwrap())
            .unwrap_or_else(|_| H256::zero());
        println!(
            "export PRICE_UPDATE_TX_HASH={} && \\",
            hex::encode(price_update_tx_hash.as_bytes()),
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
        println!("export FLASH_LOAN_SOURCE={} && \\", "1"); // TODO: Logic to determine this based on available liquidity: 1-Morpho, 2-AAVE
        println!("forge test --match-test testLiquidation -vvvvv --gas-report");
        println!("\n");

        if simulate_bundle {
            println!("\n### Simulating bundle execution with MevShare ###\n");

            let params = vec![
                Token::Tuple(vec![
                    Token::Uint(ethersU256::from_little_endian(&best.actual_debt_to_liquidate.to_le_bytes::<32>())),  // debtAmount
                    Token::Address(H160::from_slice(user_address.as_slice())),    // user
                    Token::Address(H160::from_slice(best.debt_asset.as_slice())), // debtAsset
                    Token::Address(H160::from_slice(best.collateral_asset.as_slice())), // collateral
                    Token::Uint(ethersU256::from(collateral_to_weth_fee.to::<u32>())), // collateralToWethFee
                    Token::Uint(ethersU256::from(weth_to_debt_fee.to::<u32>())), // wethToDebtFee
                    Token::Uint(ethersU256::from(1500)),               // bribePercentBps (15%)
                    Token::Uint(ethersU256::from(1)),                  // flashLoanSource
                    Token::Uint(ethersU256::from(0)),                  // aavePremium
                ])
            ];

            let function_signature = "triggerLiquidation((uint256,address,address,address,uint24,uint24,uint16,uint8,uint256))";
            let selector = &keccak256(function_signature.as_bytes())[0..4];
            let param_types = vec![
                ParamType::Tuple(vec![
                    ParamType::Uint(256),  // debtAmount
                    ParamType::Address,    // user
                    ParamType::Address,    // debtAsset
                    ParamType::Address,    // collateral
                    ParamType::Uint(24),   // collateralToWethFee
                    ParamType::Uint(24),   // wethToDebtFee
                    ParamType::Uint(16),   // bribePercentBps
                    ParamType::Uint(8),    // flashLoanSource
                    ParamType::Uint(256),  // aavePremium
                ])
            ];
            let encoded_params = encode(&params);
            let encoded = [selector, &encoded_params].concat();
            let contract_address = "0xFFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF".parse::<H160>().unwrap();
            let tx = Eip1559TransactionRequest::new()
                //.from() will definitely be required
                .to(contract_address)
                .data(encoded.to_vec());
            // TODO (Hernan) Figure out if these are required
                //.gas(U256::from(1_000_000))
                //.max_fee_per_gas(U256::from(100_000_000_000u64))
                //.max_priority_fee_per_gas(U256::from(2_000_000_000u64));
            let foxdie_tx = TypedTransaction::Eip1559(tx);
            let mev_share_service = MevShareService::new();
            mev_share_service.submit_simple_liquidation_bundle(
                if price_update_tx_hash == H256::zero() { H256::random() } else { price_update_tx_hash },
                foxdie_tx,
                true,
            ).await.unwrap();
            println!("\n### End of simulation output ###\n");
        }
    }
}
