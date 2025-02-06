use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use once_cell::sync::OnceCell;
use overlord_shared_types::UnderwaterUserEvent;
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

const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";
const AAVE_ORACLE_ADDRESS: Address = address!("0x54586bE62E3c3580375aE3723C145253060Ca0C2");
const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address =
    address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");

static PROVIDER: OnceCell<Arc<RootProvider<PubSubFrontend>>> = OnceCell::new();

#[derive(Clone)]
pub struct ProviderCache {
    initialization: Arc<Mutex<()>>,
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

/*
symbol;liqBonus;underlyingAsset
WETH;5%;0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
wstETH;6%;0x7f39c581f595b53c5cb19bd0b3f8da6c935e2ca0
WBTC;5%;0x2260fac5e5542a773aa44fbcfedf7c193bc2c599
USDC;4.5%;0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
DAI;5%;0x6b175474e89094c44da98b954eedeac495271d0f
LINK;7%;0x514910771af9ca656af840dff83e8264ecf986ca
AAVE;7.5%;0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9
cbETH;7.5%;0xbe9895146f7af43049ca1c1ae358b0541ea49704
USDT;4.5%;0xdac17f958d2ee523a2206206994597c13d831ec7
rETH;7.5%;0xae78736cd615f374d3085123a210448e74fc6393
LUSD;4.5%;0x5f98805a4e8be255a32880fdec7f6728c6568ba0
CRV;8.3%;0xd533a949740bb3306d119cc777fa900ba034cd52
MKR;8.5%;0x9f8f72aa9304c8b593d555f12ef6589cc3a579a2
SNX;8.5%;0xc011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f
BAL;8.3%;0xba100000625a3754423978a60c9317c58a424e3d
UNI;0%;0x1f9840a85d5af5bf1d1762f925bdaddc4201f984
LDO;9%;0x5a98fcbea516cf06857215779fd812ca3bef1b32
ENS;8%;0xc18360217d8f7ab5e7c516566761ea12ce7f9d72
1INCH;7.5%;0x111111111117dc0aa78b770fa6a738034120c302
FRAX;6%;0x853d955acef822db058eb8505911ed77f175b99e
GHO;0%;0x40d16fc0246ad3160ccc09b8d0d3a2cd28ae6c2f
RPL;0%;0xd33526068d116ce69f19a9ee46f0bd304f21a51f
sDAI;4.5%;0x83f20f44975d03b1b09e64809b757c47f942beea
STG;0%;0xaf5191b0de278c7286d6c7cc6ab6bb8a73ba2cd6
KNC;0%;0xdefa4e8a7bcba345f687a2f1456f5edd9ce97202
FXS;0%;0x3432b6a60d23ca0dfca7761b7ab56459d9c964d0
crvUSD;0%;0xf939e0a03fb07f59a73314e73794be0e57ac1b4e
PYUSD;7.5%;0x6c3ea9036406852006290770bedfcaba0e23a0e8
weETH;7%;0xcd5fe23c85820f7b72d0926fc9b05b43e359b7ee
osETH;7.5%;0xf1c9acdc66974dfb6decb12aa385b9cd01190e38
USDe;8.5%;0x4c9edd5852cd905f086c759e8383e09bff1e68b3
ETHx;7.5%;0xa35b1b31ce002fbf2058d22f30f95d405200a15b
sUSDe;8.5%;0x9d39a5de30e57443bff2a8307a4256c8797a3497
tBTC;7.5%;0x18084fba666a33d37592fa2633fd49a74dd93a88
cbBTC;7.5%;0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf
USDS;7.5%;0xdc035d45d973e3ec169d2276ddab16f1e407384f
rsETH;7.5%;0xa1290d69c65a6fe4df752f95823fae25cb99e5a7
LBTC;8.5%;0x8236a87084f8b84306f72007f36f2618a5634494
*/
fn get_best_debt_collateral_pair(
    user_reserve_data: Vec<UserReserveData>,
    user_health_factor: U256,
) -> Option<DebtCollateralPairInfo> {
    let mut reserve_details = HashMap::new();
    reserve_details.insert(
        address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
        ("WETH".to_string(), U256::from(10500)),
    );
    reserve_details.insert(
        address!("0x7f39c581f595b53c5cb19bd0b3f8da6c935e2ca0"),
        ("wstETH".to_string(), U256::from(10600)),
    );
    reserve_details.insert(
        address!("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599"),
        ("WBTC".to_string(), U256::from(10500)),
    );
    reserve_details.insert(
        address!("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        ("USDC".to_string(), U256::from(10450)),
    );
    reserve_details.insert(
        address!("0x6b175474e89094c44da98b954eedeac495271d0f"),
        ("DAI".to_string(), U256::from(10500)),
    );
    reserve_details.insert(
        address!("0x514910771af9ca656af840dff83e8264ecf986ca"),
        ("LINK".to_string(), U256::from(10700)),
    );
    reserve_details.insert(
        address!("0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9"),
        ("AAVE".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0xbe9895146f7af43049ca1c1ae358b0541ea49704"),
        ("cbETH".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0xdac17f958d2ee523a2206206994597c13d831ec7"),
        ("USDT".to_string(), U256::from(10450)),
    );
    reserve_details.insert(
        address!("0xae78736cd615f374d3085123a210448e74fc6393"),
        ("rETH".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0x5f98805a4e8be255a32880fdec7f6728c6568ba0"),
        ("LUSD".to_string(), U256::from(10450)),
    );
    reserve_details.insert(
        address!("0xd533a949740bb3306d119cc777fa900ba034cd52"),
        ("CRV".to_string(), U256::from(10830)),
    );
    reserve_details.insert(
        address!("0x9f8f72aa9304c8b593d555f12ef6589cc3a579a2"),
        ("MKR".to_string(), U256::from(10850)),
    );
    reserve_details.insert(
        address!("0xc011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f"),
        ("SNX".to_string(), U256::from(10850)),
    );
    reserve_details.insert(
        address!("0xba100000625a3754423978a60c9317c58a424e3d"),
        ("BAL".to_string(), U256::from(10830)),
    );
    reserve_details.insert(
        address!("0x1f9840a85d5af5bf1d1762f925bdaddc4201f984"),
        ("UNI".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0x5a98fcbea516cf06857215779fd812ca3bef1b32"),
        ("LDO".to_string(), U256::from(10900)),
    );
    reserve_details.insert(
        address!("0xc18360217d8f7ab5e7c516566761ea12ce7f9d72"),
        ("ENS".to_string(), U256::from(10800)),
    );
    reserve_details.insert(
        address!("0x111111111117dc0aa78b770fa6a738034120c302"),
        ("1INCH".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0x853d955acef822db058eb8505911ed77f175b99e"),
        ("FRAX".to_string(), U256::from(10600)),
    );
    reserve_details.insert(
        address!("0x40d16fc0246ad3160ccc09b8d0d3a2cd28ae6c2f"),
        ("GHO".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0xd33526068d116ce69f19a9ee46f0bd304f21a51f"),
        ("RPL".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0x83f20f44975d03b1b09e64809b757c47f942beea"),
        ("sDAI".to_string(), U256::from(10450)),
    );
    reserve_details.insert(
        address!("0xaf5191b0de278c7286d6c7cc6ab6bb8a73ba2cd6"),
        ("STG".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0xdefa4e8a7bcba345f687a2f1456f5edd9ce97202"),
        ("KNC".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0x3432b6a60d23ca0dfca7761b7ab56459d9c964d0"),
        ("FXS".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0xf939e0a03fb07f59a73314e73794be0e57ac1b4e"),
        ("crvUSD".to_string(), U256::from(10000)),
    );
    reserve_details.insert(
        address!("0x6c3ea9036406852006290770bedfcaba0e23a0e8"),
        ("PYUSD".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0xcd5fe23c85820f7b72d0926fc9b05b43e359b7ee"),
        ("weETH".to_string(), U256::from(10700)),
    );
    reserve_details.insert(
        address!("0xf1c9acdc66974dfb6decb12aa385b9cd01190e38"),
        ("osETH".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0x4c9edd5852cd905f086c759e8383e09bff1e68b3"),
        ("USDe".to_string(), U256::from(10850)),
    );
    reserve_details.insert(
        address!("0xa35b1b31ce002fbf2058d22f30f95d405200a15b"),
        ("ETHx".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0x9d39a5de30e57443bff2a8307a4256c8797a3497"),
        ("sUSDe".to_string(), U256::from(10850)),
    );
    reserve_details.insert(
        address!("0x18084fba666a33d37592fa2633fd49a74dd93a88"),
        ("tBTC".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf"),
        ("cbBTC".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0xdc035d45d973e3ec169d2276ddab16f1e407384f"),
        ("USDS".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0xa1290d69c65a6fe4df752f95823fae25cb99e5a7"),
        ("rsETH".to_string(), U256::from(10750)),
    );
    reserve_details.insert(
        address!("0x8236a87084f8b84306f72007f36f2618a5634494"),
        ("LBTC".to_string(), U256::from(10850)),
    );

    let mut best_pair: Option<DebtCollateralPairInfo> = None;
    let mut max_net_gain = U256::ZERO;
    let mut liquidation_close_factor;

    for reserve in user_reserve_data
        .iter()
        .filter(|r| r.scaledVariableDebt > U256::ZERO)
    {
        for collateral in user_reserve_data
            .iter()
            .filter(|r| r.scaledATokenBalance > U256::ZERO && r.usageAsCollateralEnabledOnUser)
        {
            if let (Some((debt_symbol, _)), Some((collateral_symbol, bonus))) = (
                reserve_details.get(&reserve.underlyingAsset),
                reserve_details.get(&collateral.underlyingAsset),
            ) {
                if user_health_factor <= U256::from(0.95e18) {
                    liquidation_close_factor = U256::from(1);
                } else {
                    liquidation_close_factor = U256::from(0.5);
                };
                let amount_to_liquidate = reserve.scaledVariableDebt * liquidation_close_factor;
                let bonus_multiplier = *bonus / U256::from(10000);
                let collateral_seized = amount_to_liquidate * bonus_multiplier;
                let liquidation_profit = collateral_seized - amount_to_liquidate;
                if liquidation_profit > max_net_gain {
                    max_net_gain = liquidation_profit;
                    best_pair = Some(DebtCollateralPairInfo {
                        debt_asset: reserve.underlyingAsset,
                        debt_symbol: debt_symbol.clone(),
                        debt_amount: amount_to_liquidate,
                        collateral_asset: collateral.underlyingAsset,
                        collateral_symbol: collateral_symbol.clone(),
                        collateral_seized,
                        liquidation_profit,
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

async fn process_uw_event(uw_event: UnderwaterUserEvent, provider_cache: Arc<ProviderCache>) {
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
                    let best_pair = get_best_debt_collateral_pair(
                        user_reserves_data._0,
                        uw_event.user_account_data.healthFactor,
                    )
                    .unwrap();
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
                }
                Err(e) => {
                    warn!("Failed to fetch user reserves data: {e}");
                }
            }
        }
        Err(e) => warn!("Failed to fetch UI data provider: {e}"),
    }
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
    loop {
        // Inner loop handles recv
        match socket.recv_bytes(0) {
            Ok(bytes) => match bincode::deserialize::<UnderwaterUserEvent>(&bytes) {
                Ok(uw_event) => {
                    let provider_cache = provider_cache.clone();
                    tokio::spawn(process_uw_event(uw_event, provider_cache));
                }
                Err(e) => warn!("Failed to deserialize message: {e}"),
            },
            Err(e) => warn!("Failed to receive ZMQ message: {e}"),
        }
    }
}
