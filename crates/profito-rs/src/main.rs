use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use once_cell::sync::OnceCell;
use overlord_shared_types::UnderwaterUserEvent;
use AaveProtocolDataProvider::getReserveConfigurationDataReturn;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};
use IUiPoolDataProviderV3::UserReserveData;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    AaveUIPoolDataProvider,
    "src/abis/aave_ui_pool_data_provider.json"
);

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

const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";
const AAVE_ORACLE_ADDRESS: Address = address!("0x54586bE62E3c3580375aE3723C145253060Ca0C2");
const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS: Address = address!("41393e5e337606dc3821075Af65AeE84D7688CBD");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address =
    address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");

static PROVIDER: OnceCell<Arc<RootProvider<PubSubFrontend>>> = OnceCell::new();

#[derive(Clone)]
pub struct ProviderCache {
    initialization: Arc<Mutex<()>>,
}

impl Default for ProviderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderCache {
    pub fn new() -> Self {
        Self {
            initialization: Arc::new(Mutex::new(())),
        }
    }

    pub async fn get_provider(
        &self,
    ) -> Result<Arc<RootProvider<PubSubFrontend>>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(provider) = PROVIDER.get() {
            return Ok(provider.clone());
        }
        let _lock = self.initialization.lock().await;

        if let Some(provider) = PROVIDER.get() {
            return Ok(provider.clone());
        }

        let ipc = IpcConnect::new("/tmp/reth.ipc".to_string());
        let provider = ProviderBuilder::new().on_ipc(ipc).await?;
        let provider = Arc::new(provider);

        PROVIDER
            .set(provider.clone())
            .map_err(|_| "Failed to SET provider on cache")?;
        Ok(provider)
    }
}

struct DebtCollateralPairInfo {
    debt_asset: Address,
    debt_symbol: String,
    debt_amount: U256,
    collateral_asset: Address,
    collateral_symbol: String,
    collateral_seized: U256,
    liquidation_profit: U256,
}

#[derive(Debug, Clone)]
struct ReserveConfigurationDataWithSymbol {
    symbol: String,
    data: getReserveConfigurationDataReturn,
}

type ReserveConfigurationData = HashMap<Address, ReserveConfigurationDataWithSymbol>;

async fn generate_reserve_details_by_asset(
    provider_cache: Arc<ProviderCache>,
) -> Result<ReserveConfigurationData, Box<dyn std::error::Error>> {
    let mut symbols_by_address = HashMap::new();
    symbols_by_address.insert(address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"), "WETH".to_string());
    symbols_by_address.insert(address!("7f39c581f595b53c5cb19bd0b3f8da6c935e2ca0"), "wstETH".to_string());
    symbols_by_address.insert(address!("2260fac5e5542a773aa44fbcfedf7c193bc2c599"), "WBTC".to_string());
    symbols_by_address.insert(address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"), "USDC".to_string());
    symbols_by_address.insert(address!("6b175474e89094c44da98b954eedeac495271d0f"), "DAI".to_string());
    symbols_by_address.insert(address!("514910771af9ca656af840dff83e8264ecf986ca"), "LINK".to_string());
    symbols_by_address.insert(address!("7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9"), "AAVE".to_string());
    symbols_by_address.insert(address!("be9895146f7af43049ca1c1ae358b0541ea49704"), "cbETH".to_string());
    symbols_by_address.insert(address!("dac17f958d2ee523a2206206994597c13d831ec7"), "USDT".to_string());
    symbols_by_address.insert(address!("ae78736cd615f374d3085123a210448e74fc6393"), "rETH".to_string());
    symbols_by_address.insert(address!("5f98805a4e8be255a32880fdec7f6728c6568ba0"), "LUSD".to_string());
    symbols_by_address.insert(address!("d533a949740bb3306d119cc777fa900ba034cd52"), "CRV".to_string());
    symbols_by_address.insert(address!("9f8f72aa9304c8b593d555f12ef6589cc3a579a2"), "MKR".to_string());
    symbols_by_address.insert(address!("c011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f"), "SNX".to_string());
    symbols_by_address.insert(address!("ba100000625a3754423978a60c9317c58a424e3d"), "BAL".to_string());
    symbols_by_address.insert(address!("1f9840a85d5af5bf1d1762f925bdaddc4201f984"), "UNI".to_string());
    symbols_by_address.insert(address!("5a98fcbea516cf06857215779fd812ca3bef1b32"), "LDO".to_string());
    symbols_by_address.insert(address!("c18360217d8f7ab5e7c516566761ea12ce7f9d72"), "ENS".to_string());
    symbols_by_address.insert(address!("111111111117dc0aa78b770fa6a738034120c302"), "1INCH".to_string());
    symbols_by_address.insert(address!("853d955acef822db058eb8505911ed77f175b99e"), "FRAX".to_string());
    symbols_by_address.insert(address!("40d16fc0246ad3160ccc09b8d0d3a2cd28ae6c2f"), "GHO".to_string());
    symbols_by_address.insert(address!("d33526068d116ce69f19a9ee46f0bd304f21a51f"), "RPL".to_string());
    symbols_by_address.insert(address!("83f20f44975d03b1b09e64809b757c47f942beea"), "sDAI".to_string());
    symbols_by_address.insert(address!("af5191b0de278c7286d6c7cc6ab6bb8a73ba2cd6"), "STG".to_string());
    symbols_by_address.insert(address!("defa4e8a7bcba345f687a2f1456f5edd9ce97202"), "KNC".to_string());
    symbols_by_address.insert(address!("3432b6a60d23ca0dfca7761b7ab56459d9c964d0"), "FXS".to_string());
    symbols_by_address.insert(address!("f939e0a03fb07f59a73314e73794be0e57ac1b4e"), "crvUSD".to_string());
    symbols_by_address.insert(address!("6c3ea9036406852006290770bedfcaba0e23a0e8"), "PYUSD".to_string());
    symbols_by_address.insert(address!("cd5fe23c85820f7b72d0926fc9b05b43e359b7ee"), "weETH".to_string());
    symbols_by_address.insert(address!("f1c9acdc66974dfb6decb12aa385b9cd01190e38"), "osETH".to_string());
    symbols_by_address.insert(address!("4c9edd5852cd905f086c759e8383e09bff1e68b3"), "USDe".to_string());
    symbols_by_address.insert(address!("a35b1b31ce002fbf2058d22f30f95d405200a15b"), "ETHx".to_string());
    symbols_by_address.insert(address!("9d39a5de30e57443bff2a8307a4256c8797a3497"), "sUSDe".to_string());
    symbols_by_address.insert(address!("18084fba666a33d37592fa2633fd49a74dd93a88"), "tBTC".to_string());
    symbols_by_address.insert(address!("cbb7c0000ab88b473b1f5afd9ef808440eed33bf"), "cbBTC".to_string());
    symbols_by_address.insert(address!("dc035d45d973e3ec169d2276ddab16f1e407384f"), "USDS".to_string());
    symbols_by_address.insert(address!("a1290d69c65a6fe4df752f95823fae25cb99e5a7"), "rsETH".to_string());
    symbols_by_address.insert(address!("8236a87084f8b84306f72007f36f2618a5634494"), "LBTC".to_string());

    let reserves: Vec<Address>;
    match provider_cache.get_provider().await {
        Ok(provider) => {
            let ui_data = AaveUIPoolDataProvider::new(
                AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
                provider.clone(),
            );
            match ui_data
                .getReservesList(AAVE_V3_PROVIDER_ADDRESS)
                .call()
                .await
            {
                Ok(all_reserves) => {
                    reserves = all_reserves._0;
                },
                Err(e) => {
                    return Err(format!("Failed to get reserves list to initialize reserve configuration struct: {}", e).into());
                },
            }
        },
        Err(e) => return Err(format!("Failed to get the provider to query reserves list: {}", e).into()),
    }

    let mut configuration_data: ReserveConfigurationData = HashMap::new();
    match provider_cache.get_provider().await {
        Ok(provider) => {
            let aave_config = AaveProtocolDataProvider::new(
                AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS,
                provider.clone(),
            );
            
            for reserve_address in reserves {
                match aave_config
                    .getReserveConfigurationData(reserve_address)
                    .call()
                    .await
                {
                    Ok(reserve_config) => {
                        configuration_data.insert(reserve_address, ReserveConfigurationDataWithSymbol {symbol: symbols_by_address.get(&reserve_address).cloned().unwrap_or_default(), data: reserve_config});
                    },
                    Err(e) => return Err(format!("Failed to get reserve configuration data for asset {}: {}", reserve_address, e).into()),
                }
            }
        },
        Err(e) => return Err(format!("Failed to get provider to query reserve configuration: {}", e).into()),
    }
    Ok(configuration_data)
}

fn get_best_debt_collateral_pair(
    reserves_configuration: ReserveConfigurationData,
    user_reserve_data: Vec<UserReserveData>,
    user_health_factor: U256,
) -> Option<DebtCollateralPairInfo> {
    let mut best_pair: Option<DebtCollateralPairInfo> = None;
    let mut max_net_gain = U256::from(0);
    let mut liquidation_close_factor;
    for reserve in user_reserve_data
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::from(0))
    {
        for collateral in user_reserve_data
            .iter()
            .filter(|r| r.scaledATokenBalance > U256::from(0) && r.usageAsCollateralEnabledOnUser)
        {
            if let (Some(debt_config), Some(collateral_config)) = (
                reserves_configuration.get(&reserve.underlyingAsset),
                reserves_configuration.get(&collateral.underlyingAsset),
            ) {
                let debt_symbol = &debt_config.symbol;
                let collateral_symbol = &collateral_config.symbol;
                let bonus = &collateral_config.data.liquidationBonus;
                // source: https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L50-L68
                if user_health_factor <= U256::from(0.95e18) {
                    liquidation_close_factor = U256::from(1e4);
                } else {
                    liquidation_close_factor = U256::from(0.5e4);
                };
                // This mimics percentMul at https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/math/PercentageMath.sol#L25
                // the same way it's used here: https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L379
                let actual_debt_to_liquidate = ((reserve.scaledVariableDebt * liquidation_close_factor) + U256::from(0.5e4)) / U256::from(1e4);

                // under this line, we don't know what we're doing (YET)
                let bonus_multiplier = (*bonus * U256::from(1e18)) / U256::from(10000);
                let collateral_seized = (actual_debt_to_liquidate * bonus_multiplier) / U256::from(1e18);
                let liquidation_profit = collateral_seized - actual_debt_to_liquidate;
                info!("debt ({}) | close_factor ({}) | bonus ({}) | bonus_mult ({}) | collateral_seized ({})", actual_debt_to_liquidate, liquidation_close_factor, *bonus, bonus_multiplier, collateral_seized);
                if liquidation_profit > max_net_gain {
                    max_net_gain = liquidation_profit;
                    best_pair = Some(DebtCollateralPairInfo {
                        debt_asset: reserve.underlyingAsset,
                        debt_symbol: debt_symbol.clone(),
                        debt_amount: actual_debt_to_liquidate / U256::from(1e18),
                        collateral_asset: collateral.underlyingAsset,
                        collateral_symbol: collateral_symbol.clone(),
                        collateral_seized: collateral_seized / U256::from(1e18),
                        liquidation_profit: liquidation_profit / U256::from(1e18),
                    });
                }
            }
        }
    }
    best_pair
}

fn _setup_logging() {
    let log_file = rolling::RollingFileAppender::new(
        Rotation::DAILY,
        "/var/log/overlord-rs",
        "profito-rs.log",
    );
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .init();
}

pub async fn convert_eth_to_asset(
    aave_oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, Arc<RootProvider<PubSubFrontend>>>,
    asset_address: Address,
    amount_in_eth: U256,
) -> Result<U256, Box<dyn std::error::Error>> {
    //TODO(Hernan) These values could be cached and be valid throughout the same trace_id
    let weth_address = address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    let eth_price_usd = aave_oracle.getAssetPrice(weth_address).call().await?._0;
    let asset_price_usd = aave_oracle.getAssetPrice(asset_address).call().await?._0;
    let amount_in_asset = amount_in_eth * eth_price_usd / asset_price_usd;
    Ok(amount_in_asset)
}

async fn process_uw_event(uw_event: UnderwaterUserEvent, reserves_configuration: ReserveConfigurationData, provider_cache: Arc<ProviderCache>) -> Result<(), Box<dyn std::error::Error>> {
    let process_uw_event_timer = Instant::now();
    match provider_cache.get_provider().await {
        Ok(provider) => {
            let ui_data = AaveUIPoolDataProvider::new(
                AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
                provider.clone(),
            );
            let aave_oracle: AaveOracle::AaveOracleInstance<
                PubSubFrontend,
                Arc<RootProvider<PubSubFrontend>>,
            > = AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());

            match ui_data
                .getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, uw_event.address)
                .call()
                .await
            {
                Ok(user_reserves_data) => {
                    if let Some(best_pair) = get_best_debt_collateral_pair(
                        reserves_configuration,
                        user_reserves_data._0,
                        uw_event.user_account_data.healthFactor,
                    ) {
                        info!(
                            "opportunity analysis for {}: repay {} {} to get raw/net {}/{} {} ({}/{} USD) as reward - {:?}ms", 
                            uw_event.address,
                            convert_eth_to_asset(aave_oracle.clone(), best_pair.debt_asset, best_pair.debt_amount).await.unwrap(),
                            best_pair.debt_symbol,
                            best_pair.collateral_seized,
                            best_pair.liquidation_profit,
                            best_pair.collateral_symbol,
                            convert_eth_to_asset(aave_oracle.clone(), best_pair.collateral_asset, best_pair.collateral_seized).await.unwrap(),
                            convert_eth_to_asset(aave_oracle.clone(), best_pair.collateral_asset, best_pair.liquidation_profit).await.unwrap(),
                            process_uw_event_timer.elapsed(),
                        );
                    } else {
                        warn!("No profitable liquidation pair found for {}", uw_event.address);
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch user reserves data: {e}");
                }
            }
        }
        Err(e) => warn!("Failed to get the provider for uw processing: {e}"),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    _setup_logging();
    info!("Starting Profito RS");
    let provider_cache = Arc::new(ProviderCache::new());
    let context = zmq::Context::new();
    let socket = context.socket(zmq::PULL).unwrap();
    if let Err(e) = socket.bind(PROFITO_INBOUND_ENDPOINT) {
        error!("Failed to bind ZMQ socket: {e}");
        return;
    }
    info!(
        "Listening for health factor alerts on {}",
        PROFITO_INBOUND_ENDPOINT
    );
    let reserves_configuration = generate_reserve_details_by_asset(provider_cache.clone())
        .await
        .unwrap_or_else(|e| {
            error!("Failed to initialize reserve configuration: {}", e);
            std::process::exit(1);
        });
    loop {
        match socket.recv_bytes(0) {
            Ok(bytes) => match bincode::deserialize::<UnderwaterUserEvent>(&bytes) {
                Ok(uw_event) => {
                    let provider_cache = provider_cache.clone();
                    let reserves_configuration = reserves_configuration.clone();
                    tokio::spawn(async move {
                        if let Err(e) = process_uw_event(uw_event, reserves_configuration, provider_cache).await {
                            warn!("Failed to process underwater event: {e}");
                        }
                    });
                }
                Err(e) => warn!("Failed to deserialize message: {e}"),
            },
            Err(e) => warn!("Failed to receive ZMQ message: {e}"),
        }
    }
}
