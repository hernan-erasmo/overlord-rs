use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use chrono::Local;
use futures::{
    future::join_all,
    stream::{self, StreamExt},
};
use overlord_shared_types::{PriceUpdateBundle, WhistleblowerEventType, WhistleblowerUpdate};
use pool::AaveV3Pool;
use serde_json::json;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{self, BufRead},
    str::FromStr,
};
use tokio::{sync::RwLock, task, time::Instant};
use tracing::{error, info, warn};

use overlord_shared_types::sol_bindings::{
    pool::AaveV3Pool,
    AaveUIPoolDataProvider,
};
use vega_rs::user_reserve_cache::{
    get_positions_by_user, load_addresses_from_file, UserPosition, generate_user_by_position_by_asset, PositionType
};

pub type UserAddress = Address;
type ReserveAddress = Address;
type ChainlinkContractAddress = Address;

const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address =
    address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");
const BUCKETS: usize = 64;
const CONCURRENCY_LIMIT: usize = 1024;

#[derive(Debug)]
struct UserReservesCacheInitStats {
    input_user_addresses: usize,
    most_supplied_reserve: String,
    most_supplied_reserve_count: usize,
    most_borrowed_reserve: String,
    most_borrowed_reserve_count: usize,
    total_user_addresses_in_cache: usize,
}

#[derive(Debug, Clone)]
pub struct AaveReserveInfo {
    pub symbol: String,
    pub reserve_address: ReserveAddress,
}

const ADDRESSES_FILE_ENV: &str = "/home/hernan/projects/overlord-rs/data/vega/addresses_20250413151425_40489.txt";

pub async fn optimized_get_positions_by_user(
    addresses: &Vec<UserAddress>,
    provider: &RootProvider<PubSubFrontend>,
) -> Result<HashMap<UserAddress, Vec<UserPosition>>, Box<dyn std::error::Error>> {
    let ui_data =
        AaveUIPoolDataProvider::new(AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let aave_pool = AaveV3Pool::new(address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"), provider.clone());
    
    let aggregate_results: Vec<(UserAddress, Vec<UserPosition>)> = stream::iter(addresses.into_iter())
        .map(|address| {
        let ui_data = ui_data.clone();
        let aave_pool = aave_pool.clone();
        async move {
            // First check if user has any debt
            let has_debt = match aave_pool.getUserAccountData(*address).call().await {
                Ok(data) => data.totalDebtBase > U256::ZERO,
                Err(e) => {
                    println!("Couldn't get user account data: {:?}", e);
                    return None;
                }
            };

            if !has_debt {
                return None;
            }

            // Then get user reserves
            match ui_data
                .getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, *address)
                .call()
                .await
            {
                Ok(data) => {
                    let user_positions: Vec<UserPosition> = data
                        ._0
                        .iter()
                        .map(|d| UserPosition {
                            scaled_atoken_balance: d.scaledATokenBalance,
                            usage_as_collateral_enabled_on_user: d
                                .usageAsCollateralEnabledOnUser,
                            scaled_variable_debt: d.scaledVariableDebt,
                            underlying_asset: d.underlyingAsset,
                        })
                        .collect();

                    if !user_positions.is_empty() {
                        Some((*address, user_positions))
                    } else {
                        None
                    }
                }
                Err(e) => {
                    warn!("Couldn't calculate address reserves: {:?}", e);
                    None
                }
            }
        }
    })
    .buffer_unordered(CONCURRENCY_LIMIT)
    .filter_map(|result| std::future::ready(result)) // Remove Nones
    .collect()
    .await;

    let mut raw_results = HashMap::new();
    for (address, positions) in aggregate_results {
        raw_results.insert(address, positions);
    }
    Ok(raw_results)
}

async fn current_way_of_doing_things() -> Result<HashMap<UserAddress, Vec<UserPosition>>, Box<dyn std::error::Error>> {
    println!("Loading addresses from {}", ADDRESSES_FILE_ENV);

    // Step 1: read addresses from file
    let user_addresses = match load_addresses_from_file(ADDRESSES_FILE_ENV) {
        Ok(addresses) => addresses,
        Err(e) => {
            return Err(format!("Error loading addresses: {}", e).into());
        },
    };
    println!("Loaded {} addresses", user_addresses.len());

    // Step 2: bucketize addresses
    let user_addresses: Vec<UserAddress> = user_addresses
    .into_iter()
    .collect::<HashSet<_>>()
    .into_iter()
    .collect();
    let user_addresses_buckets: Vec<Vec<UserAddress>> = user_addresses
        .chunks(64)
        .map(|chunk| chunk.to_vec())
        .collect();

    // Step 3: setup the provider
    let ipc_path = "/tmp/reth.ipc";
    let ipc = IpcConnect::new(ipc_path.to_string());
    let provider = match ProviderBuilder::new().on_ipc(ipc).await {
        Ok(p) => p,
        Err(e) => {
            return Err(format!("Failed to create provider: {}", e).into());
        }
    };

    let get_positions_by_user_perf = Instant::now();
    // Step 4: call getUserReservesData on each address of each bucket
    let positions_by_user: HashMap<UserAddress, Vec<UserPosition>> =
        match get_positions_by_user(&user_addresses_buckets, &provider).await {
            Ok(positions) => positions,
            Err(e) => {
                return Err(format!("Failed to get positions by user: {}", e).into());
            }
        };
    println!(
        "old get_positions_by_user took: {} ms",
        get_positions_by_user_perf.elapsed().as_millis()
    );
    Ok(positions_by_user)
}

async fn new_way_of_doing_things() -> Result<HashMap<UserAddress, Vec<UserPosition>>, Box<dyn std::error::Error>> {
    println!("Loading addresses from {}", ADDRESSES_FILE_ENV);

    // Step 1: read addresses from file
    let user_addresses = match load_addresses_from_file(ADDRESSES_FILE_ENV) {
        Ok(addresses) => addresses,
        Err(e) => {
            return Err(format!("Error loading addresses: {}", e).into());
        },
    };
    println!("Loaded {} addresses", user_addresses.len());

    // Step 2: bucketize addresses
    let user_addresses: Vec<UserAddress> = user_addresses
    .into_iter()
    .collect::<HashSet<_>>()
    .into_iter()
    .collect();
    let user_addresses_buckets: Vec<Vec<UserAddress>> = user_addresses
        .chunks(64)
        .map(|chunk| chunk.to_vec())
        .collect();

    // Step 3: setup the provider
    let ipc_path = "/tmp/reth.ipc";
    let ipc = IpcConnect::new(ipc_path.to_string());
    let provider = match ProviderBuilder::new().on_ipc(ipc).await {
        Ok(p) => p,
        Err(e) => {
            return Err(format!("Failed to create provider: {}", e).into());
        }
    };

    let get_positions_by_user_perf = Instant::now();
    // NEW PREPROCESSING STEP: optimized_get_positions_by_user only get the addresses that have debt, by calling getUserAccountData first
    let positions_by_user: HashMap<UserAddress, Vec<UserPosition>> =
        match optimized_get_positions_by_user(&user_addresses, &provider).await {
            Ok(positions) => positions,
            Err(e) => {
                return Err(format!("Failed to get positions by user: {}", e).into());
            }
        };
    println!(
        "new get_positions_by_user took: {} ms",
        get_positions_by_user_perf.elapsed().as_millis()
    );
    Ok(positions_by_user)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let old_way_processing = Instant::now();
    let old_positions_by_user = current_way_of_doing_things().await.unwrap();
    println!("Old way processing took: {} ms", old_way_processing.elapsed().as_millis());
    println!("Old way positions by user len = {}", old_positions_by_user.len());
    let old_users_by_positions_by_asset = generate_user_by_position_by_asset(old_positions_by_user.clone());
    // create a list with the keys of old_users_by_positions_by_asset and sort it
    let mut old_keys: Vec<String> = old_users_by_positions_by_asset.keys().map(|k| k.to_string()).collect();
    old_keys.sort();
    // iterate over the old_keys and print the keys and the values
    let mut old_full_set_of_addresses: HashSet<Address> = HashSet::new();
    for old_key in old_keys.iter() {
        let address = Address::from_str(old_key).unwrap();
        let user_positions = old_users_by_positions_by_asset.get(&address).unwrap();
        //println!("Asset: {}", old_key);
        let empty_vec: Vec<Address> = Vec::new();
        for position_type in vec![PositionType::Collateral, PositionType::Borrowed] {
            let user_position = user_positions.get(&position_type).unwrap_or(&empty_vec);
            old_full_set_of_addresses.extend(user_position.iter().cloned());
            //println!("  Position type: {:?}, users: {}", position_type, user_position.len());
        }
    }
    println!("old user positions by asset contains {} different users", old_full_set_of_addresses.len());
    println!("----------------------------------------\n");

    let new_way_processing = Instant::now();
    let new_positions_by_user = new_way_of_doing_things().await.unwrap();
    println!("New way processing took: {} ms", new_way_processing.elapsed().as_millis());
    println!("New way positions by user len = {}", new_positions_by_user.len());
    let new_users_by_positions_by_asset = generate_user_by_position_by_asset(new_positions_by_user.clone());
    // create a list with the keys of new_users_by_positions_by_asset and sort it
    let mut new_keys: Vec<String> = new_users_by_positions_by_asset.keys().map(|k| k.to_string()).collect();
    new_keys.sort();

    // iterate over the new_keys and print the keys and the values
    let mut new_full_set_of_addresses: HashSet<Address> = HashSet::new();
    for new_key in new_keys.iter() {
        let address = Address::from_str(new_key).unwrap();
        let user_positions = new_users_by_positions_by_asset.get(&address).unwrap();
        //println!("Asset: {}", new_key);
        let empty_vec: Vec<Address> = Vec::new();
        for position_type in vec![PositionType::Collateral, PositionType::Borrowed] {
            let user_position = user_positions.get(&position_type).unwrap_or(&empty_vec);
            new_full_set_of_addresses.extend(user_position.iter().cloned());
            //println!("  Position type: {:?}, users: {}", position_type, user_position.len());
        }
    }
    println!("new user positions by asset contains {} different users", new_full_set_of_addresses.len());
    Ok(())
}
