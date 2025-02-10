use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use IUiPoolDataProviderV3::UserReserveData;
use std::env;

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

const AAVE_ORACLE_ADDRESS: Address = address!("0x54586bE62E3c3580375aE3723C145253060Ca0C2");
const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS: Address =
    address!("41393e5e337606dc3821075Af65AeE84D7688CBD");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address =
    address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");

/// Get's the list of user reserves, but only returns those that the user has at least some debt or collateral and,
/// for the later, the ones that are allowed to be used as collateral
async fn get_user_reserves_data(provider: RootProvider<PubSubFrontend>, user_address: Address) -> Vec<UserReserveData> {
    let ui_data = AaveUIPoolDataProvider::new(
        AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
        provider.clone(),
    );
    let user_reserves_data: Vec<IUiPoolDataProviderV3::UserReserveData>;

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
    for reserve in user_reserves_data.iter() {
        println!("ASSET = {}, DEBT = {}, COLLATERAL = {}, USAGE_AS_COLLATERAL = {}", reserve.underlyingAsset, reserve.scaledVariableDebt, reserve.scaledATokenBalance, reserve.usageAsCollateralEnabledOnUser);
    }
}
