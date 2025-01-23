use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use chrono::Local;
use futures::future::join_all;
use overlord_shared_types::{PriceUpdateBundle, WhistleblowerEventType, WhistleblowerUpdate};
use std::{
    collections::HashMap, error::Error, fs::File, io::{self, BufRead}, str::FromStr
};
use tokio::{
    sync::RwLock,
    task,
    time::Instant
};
use std::collections::HashSet;
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use tracing::{error, info, warn};

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AaveUIPoolDataProvider,
    "src/abis/aave_ui_pool_data_provider.json"
);

type UserAddress = Address;
type ReserveAddress = Address;
type ChainlinkContractAddress = Address;

const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address = address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");
const BUCKETS: usize = 64;

#[derive(Debug, Eq, PartialEq, Hash)]
enum PositionType {
    BORROWED,
    COLLATERAL,
}

#[derive(Debug)]
struct UserPosition {
    scaled_atoken_balance: U256,
    usage_as_collateral_enabled_on_user: bool,
    scaled_variable_debt: U256,
    underlying_asset: ReserveAddress,
}

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
struct AaveReserveInfo {
    symbol: String,
    reserve_address: ReserveAddress,
}

pub struct UserReservesCache {
    user_reserves_cache: RwLock<HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>>>,

    /// Given a Chainlink contract adddress that a price update forwarded an update to,
    /// it returns a vector of all ReserveAddresses from AAVE whose prices were affected by the update.
    /// (either directly, or indirectly as is the case of assets with a price computed based on other assets)
    chainlink_address_to_asset: HashMap<ChainlinkContractAddress, Vec<AaveReserveInfo>>,
}

impl UserReservesCache {
    pub fn new() -> Self {
        UserReservesCache {
            user_reserves_cache: RwLock::new(HashMap::new()),
            chainlink_address_to_asset: HashMap::new(),
        }
    }

    /// The user cache is a mapping from assets to (eventually) users that are either borrowing or
    /// supplying those assets. On each whistleblower-rs update, this method is called and it determines
    /// whether the user cache must be updated depending on it's event type. Liquidations, borrows,
    /// supplyings, and repayments are the events that can affect whether a user is borrowing or supplying
    /// a given asset.
    pub async fn update_cache(&mut self, wb_update: &WhistleblowerUpdate) -> Result<(), Box<dyn std::error::Error>> {
        let update_type = &wb_update.event_details.event;
        let affected_user_index;

        #[allow(unreachable_patterns)]  // so rustc doesn't complain about the default case
        match update_type {
            WhistleblowerEventType::Repay |
            WhistleblowerEventType::Borrow |
            WhistleblowerEventType::Supply => {
                affected_user_index = 1;
            }
            WhistleblowerEventType::LiquidationCall => {
                affected_user_index = 2;
            }
            _ => {
                warn!("Update type {:?} shouldn't trigger a user cache update. Skipping.", update_type);
                return Ok(());
            }
        }

        let affected_user_arg = match wb_update.event_details.args.get(affected_user_index) {
            Some(arg) => arg,
            None => {
                warn!("Failed to get affected user arg at index {}. Args: {:?}", affected_user_index, wb_update.event_details.args);
                return Err("Missing affected user argument".into());
            }
        };

        let affected_user = match Address::from_str(affected_user_arg) {
            Ok(address) => address,
            Err(e) => {
                warn!("Failed to parse affected user address: {}", e);
                return Err(e.into());
            }
        };

        self._drop_user_from_cache(&affected_user).await;
        match self._add_user_to_cache(&affected_user).await {
            Ok(_) => (),
            Err(e) => {
                warn!("Failed to add user to cache: {}", e);
                return Err(e);
            }
        };
        info!("Cache updated, all write locks released.");
        Ok(())
    }

    /// Removes the user from the cache. This is done by iterating over all the assets in the cache and
    /// removing the user from the list of users that are borrowing or supplying that asset.
    async fn _drop_user_from_cache(&mut self, user: &UserAddress) {
        eprintln!("Dropping cache occurrences for user {}", user);
        let mut cache = self.user_reserves_cache.write().await;
        for (_asset, users_by_position) in cache.iter_mut() {
            for (_position_type, users) in users_by_position.iter_mut() {
                users.retain(|u| u != user);
            }
        }
    }

    /// This function
    /// 1. calls getUserReservesData for the given user address
    /// 2. parses the result into a list of UserPosition
    /// 3. for each UserPosition, it updates the cache by adding the user to the list of users that are borrowing
    ///   or supplying that asset.
    async fn _add_user_to_cache(&mut self, user: &UserAddress) -> Result<(), Box<dyn std::error::Error>> {
        let user_address = user.clone();
        let ipc_path = "/tmp/reth.ipc";
        let ipc = IpcConnect::new(ipc_path.to_string());
        let provider = match ProviderBuilder::new().on_ipc(ipc).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to create provider: {}", e);
                return Err(e.into());
            }
        };
        let ui_data = AaveUIPoolDataProvider::new(
            AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
            provider.clone()
        );
        let mut user_positions: Vec<UserPosition> = vec![];
        eprintln!("Getting reserve data information for user {}", user_address);
        let result = ui_data.getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, user_address.clone()).call().await;
        match result {
            Ok(data) => {
                user_positions = data._0.iter()
                    .map(|d| UserPosition {
                        scaled_atoken_balance: d.scaledATokenBalance,
                        usage_as_collateral_enabled_on_user: d.usageAsCollateralEnabledOnUser,
                        scaled_variable_debt: d.scaledVariableDebt,
                        underlying_asset: d.underlyingAsset,
                    }).collect();
            }
            Err(e) => {
                warn!("Couldn't calculate address reserves: {:?}", e);
                return Err(e.into());
            }
        }
        let mut cache = self.user_reserves_cache.write().await;
        for position in user_positions {
            let users_by_position = match cache.get_mut(&position.underlying_asset) {
                Some(ubp) => ubp,
                None => {
                    warn!("Underlying asset {} not found in user_reserves_cache", position.underlying_asset);
                    return Err("Asset not found in cache".into());
                }
            };
            if position.scaled_variable_debt > U256::ZERO {
                let users_vector = users_by_position.entry(PositionType::BORROWED).or_insert_with(Vec::new);
                users_vector.push(user_address.clone());
            }
            if position.usage_as_collateral_enabled_on_user && position.scaled_atoken_balance > U256::ZERO {
                let users_vector = users_by_position.entry(PositionType::COLLATERAL).or_insert_with(Vec::new);
                users_vector.push(user_address.clone());
            }
        }
        Ok(())
    }

    pub async fn initialize_cache(&mut self, addresses_file: &str, chainlink_addresses_file: &str) -> Result<Vec<Vec<UserAddress>>, Box<dyn Error>> {
        info!("Initializing UserReservesCache");
        // Step 0: Initialize stats
        let mut stats = UserReservesCacheInitStats {
            input_user_addresses: 0,
            most_supplied_reserve: String::new(),
            most_supplied_reserve_count: 0,
            most_borrowed_reserve: String::new(),
            most_borrowed_reserve_count: 0,
            total_user_addresses_in_cache: 0,
        };

        // Step 1: Load and prepare user and contract addresses
        self.chainlink_address_to_asset = match load_chainlink_addresses(&chainlink_addresses_file) {
            Ok(addresses) => addresses,
            Err(e) => {
                error!("Failed to load chainlink addresses: {}", e);
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Failed to load chainlink addresses")));
            }
        };
        let user_addresses: Vec<UserAddress> = match load_addresses_from_file(&addresses_file) {
            Ok(addresses) => addresses,
            Err(e) => {
                error!("Failed to load addresses from file: {}", e);
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Failed to load addresses from file")));
            }
        };
        stats.input_user_addresses = user_addresses.len();
        let user_addresses: Vec<UserAddress> = user_addresses.into_iter().collect::<HashSet<_>>().into_iter().collect();
        let user_addresses_buckets: Vec<Vec<UserAddress>> = user_addresses.chunks(BUCKETS).map(|chunk| chunk.to_vec()).collect();

        // Step 2: Setup the provider
        let ipc_path = "/tmp/reth.ipc";
        let ipc = IpcConnect::new(ipc_path.to_string());
        let provider = ProviderBuilder::new().on_ipc(ipc).await?;

        // Step 3: Get information about user positions
        info!("Getting user positions");
        let positions_by_user: HashMap<UserAddress, Vec<UserPosition>> = match get_positions_by_user(&user_addresses_buckets, &provider).await {
            Ok(positions) => positions,
            Err(e) => {
                error!("Failed to get positions by user: {}", e);
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Failed to get positions by user")));
            }
        };

        // Step 4: Re-arrange the information into users by position by asset
        let user_by_position_by_asset: HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>> = generate_user_by_position_by_asset(positions_by_user);
        self.user_reserves_cache = RwLock::new(user_by_position_by_asset);

        self._collect_and_dump_cache_init_stats(&mut stats).await?;
        info!(initialization_stats = ?stats, "Cache init complete");
        Ok(user_addresses_buckets)
    }

    async fn _collect_and_dump_cache_init_stats(&mut self, stats: &mut UserReservesCacheInitStats) -> Result<(), Box<dyn Error>> {
        let mut most_borrowed: (String, usize) = (String::new(), 0);
        let mut most_supplied: (String, usize) = (String::new(), 0);
        let timestamp = Local::now().format("%Y%m%d").to_string();
        let init_output_file_path = format!("user_reserves_cache_{}.json", timestamp);
        let mut init_output_file = OpenOptions::new().create(true).write(true).truncate(true).open(&init_output_file_path)?;
        let mut json_data = vec![];
        let cache = self.user_reserves_cache.read().await;
        for (asset, users_by_position) in cache.iter() {
            let borrowed_for_asset = users_by_position.get(&PositionType::BORROWED).unwrap_or(&vec![]).len();
            let used_as_collateral = users_by_position.get(&PositionType::COLLATERAL).unwrap_or(&vec![]).len();
            stats.total_user_addresses_in_cache += borrowed_for_asset + used_as_collateral;
            if borrowed_for_asset > most_borrowed.1 {
                most_borrowed = (asset.to_string().clone(), borrowed_for_asset);
            }
            if used_as_collateral > most_supplied.1 {
                most_supplied = (asset.to_string().clone(), used_as_collateral);
            }
            let mut positions = vec![];
            for (position_type, users) in users_by_position.iter() {
                let user_list: Vec<String> = users.iter().map(|user| user.to_string()).collect();
                positions.push(json!({
                    "position_type": format!("{:?}", position_type),
                    "users": user_list,
                }));
            }
            json_data.push(json!({
                "asset": asset.to_string(),
                "borrowed_for_asset": borrowed_for_asset,
                "used_as_collateral": used_as_collateral,
                "positions": positions,
            }));
        }
        let json_output = json!({
            "timestamp": timestamp,
            "data": json_data,
        });
        init_output_file.write_all(serde_json::to_string_pretty(&json_output)?.as_bytes())?;
        stats.most_borrowed_reserve = most_borrowed.0;
        stats.most_borrowed_reserve_count = most_borrowed.1;
        stats.most_supplied_reserve = most_supplied.0;
        stats.most_supplied_reserve_count = most_supplied.1;
        Ok(())
    }

    /// Returns the user addresses affected by this price update bundle
    pub async fn get_candidates_for_bundle(&mut self, bundle: Option<&PriceUpdateBundle>) -> Vec<Vec<Address>> {
        let bundle_processing = Instant::now();
        let empty_response = vec![vec![]];
        if bundle.is_none() {
            return empty_response;
        }
        let mut duplicate_candidates: Vec<UserAddress> = vec![];
        let forwarded_to_address = &bundle.unwrap().forward_to;

        let affected_reserves = self._calculate_affected_reserves(forwarded_to_address);
        if affected_reserves.is_empty() {
            return empty_response;
        }

        /*
        Each entry of the user_reserves_cache has two keys (PositionType::BORROWED and PositionType::COLLATERAL) and each
        maps to a vector of UserAddress. We want to collect all the UserAddress that are associated with the each
        reserve address that we found in the previous step.
        */
        let mut user_count_for_reserve: HashMap<String, usize> = HashMap::new();
        let cache = self.user_reserves_cache.read().await;
        for affected_reserve in affected_reserves {
            if let Some(users_by_position) = cache.get(&affected_reserve.reserve_address) {
                // Two things are done in the next block:
                // 1. Populates the `duplicate_candidates` vector. The map() makes two iterations, the first one over
                //    BORROWERS and the second over SUPPLIERS of the given asset. Then both of these user lists are
                //    added to `duplicate_candidates`, whose name reflect the fact that, given that a user can be
                //    borrowing and supplying an asset at the same time, we can end up with a list of users where
                //    some are listed twice.
                // 2. Stores the sum of users that are BORROWING and SUPPLYING a given asset into `total_users_for_reserve`
                let total_users_for_reserve: usize = users_by_position.values().map(|users| {
                    duplicate_candidates.extend(users.iter().cloned());
                    users.len()
                }).sum();
                *user_count_for_reserve.entry(affected_reserve.symbol.clone()).or_insert(0) += total_users_for_reserve;
            }
        }

        if user_count_for_reserve.is_empty() {
            // It can happen that some assets are affected by the price update (so this won't be caught by previous
            // early returns), but there are neither borrowers nor suppliers for them.
            // There's no need to continue processing over those.
            return empty_response;
        }

        let log_message = user_count_for_reserve.iter()
            .map(|(symbol, count)| format!("({}, {})", symbol, count))
            .collect::<Vec<_>>()
            .join(", ");

        let unique_candidates: HashSet<UserAddress> = duplicate_candidates.clone().into_iter().collect();
        let candidate_buckets: Vec<Vec<UserAddress>> = unique_candidates
            .clone()
            .into_iter()
            .collect::<Vec<_>>()
            .chunks(BUCKETS)
            .map(|chunk| chunk.to_vec())
            .collect();
        let bundle_processing_elapsed = bundle_processing.elapsed().as_millis();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        eprintln!("{} | bundle:{} results | {} ms | {} total | {} unique | {:?} buckets | {}",
            now,
            &bundle.unwrap().trace_id,
            bundle_processing_elapsed,
            duplicate_candidates.len(),
            unique_candidates.len(),
            candidate_buckets.iter().map(|bucket| bucket.len()).collect::<Vec<_>>(),
            log_message
        );
        candidate_buckets
    }

    /// The forwarded_to address represents the Chainlink address to which the price update was directed to.
    /// The asset_to_contract_address_mapping file has information that helps us map forwarded_to addresses to
    /// Aave reserve addresses, which are the keys for our user_reserves_cache structure.
    ///
    /// So then, we first collect all the reserve addresses that are associated with the forwarded_to address.
    /// It will be a vector of at least 1 address, but it could be more if the same Chainlink address is associated
    /// with multiple Aave reserves.
    fn _calculate_affected_reserves(&mut self, forwarded_to_address: &Address) -> Vec<AaveReserveInfo> {
        let mut affected_reserve_addresses: Vec<AaveReserveInfo> = vec![];
        if let Some(affected_reserve_info) = self.chainlink_address_to_asset.get(forwarded_to_address) {
            for reserve in affected_reserve_info {
                affected_reserve_addresses.push((*reserve).clone());
            }
        }
        affected_reserve_addresses
    }
}

fn load_chainlink_addresses(filepath: &str) -> Result<HashMap<ChainlinkContractAddress, Vec<AaveReserveInfo>>, Box<dyn Error>> {
    let mut chainlink_addresses: HashMap<ChainlinkContractAddress, Vec<AaveReserveInfo>> = HashMap::new();
    let file = File::open(filepath)?;
    let mut lines = io::BufReader::new(file).lines();
    lines.next();  // Skip the header line
    for line in lines {
        let line = line?;
        let parts: Vec<&str> = line.split(",").collect();
        let symbol = parts[0].to_string();
        let reserve_address = match Address::from_str(parts[1]) {
            Ok(addr) => addr,
            Err(e) => {
                error!("Failed to parse AAVE reserve address: {}", e);
                return Err(Box::new(e));
            }
        };
        let chainlink_address = match Address::from_str(parts[2]) {
            Ok(addr) => addr,
            Err(e) => {
                error!("Failed to parse chainlink address: {}", e);
                return Err(Box::new(e));
            }
        };
        let reserve_info = AaveReserveInfo {
            symbol,
            reserve_address,
        };
        chainlink_addresses.entry(chainlink_address)
            .or_insert_with(Vec::new)
            .push(reserve_info);
    }
    info!("Loaded {} chainlink addresses.", chainlink_addresses.len());
    Ok(chainlink_addresses)
}

fn load_addresses_from_file(filepath: &str) -> Result<Vec<UserAddress>, Box<dyn Error>> {
    let mut addresses: Vec<UserAddress> = Vec::new();
    let file = File::open(filepath)?;
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        let address = match Address::from_str(str::trim(&line)) {
            Ok(addr) => addr,
            Err(e) => {
                warn!("Failed to parse address '{}': {}", line, e);
                return Err(Box::new(e));
            }
        };
        addresses.push(address);
    }
    eprintln!("Loaded {} user addresses.", addresses.len());
    Ok(addresses)
}

async fn get_positions_by_user(
    address_buckets: &Vec<Vec<UserAddress>>,
    provider: &RootProvider<PubSubFrontend>,
) -> Result<HashMap<UserAddress, Vec<UserPosition>>, Box<dyn Error>> {
    let mut tasks = vec![];
    let ui_data = AaveUIPoolDataProvider::new(
        AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
        provider.clone()
    );
    for bucket in address_buckets.clone() {
        let ui_data = ui_data.clone();
        let task = task::spawn(async move {
            let mut results: HashMap<UserAddress, Vec<UserPosition>> = HashMap::new();
            for address in bucket {
                // returns (UserReserveData[] memory, uint8)
                let result = ui_data.getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, address.clone()).call().await;
                match result {
                    Ok(data) => {
                        let user_positions: Vec<UserPosition> = data._0.iter()
                            .map(|d| UserPosition {
                                scaled_atoken_balance: d.scaledATokenBalance,
                                usage_as_collateral_enabled_on_user: d.usageAsCollateralEnabledOnUser,
                                scaled_variable_debt: d.scaledVariableDebt,
                                underlying_asset: d.underlyingAsset,
                            }).collect();
                        if !user_positions.is_empty() {
                            results.insert(address.clone(), user_positions);
                        }
                    }
                    Err(e) => warn!("Couldn't calculate address reserves: {:?}", e),
                }
            }
            results
        });
        tasks.push(task);
    }
    let aggregate_results: Vec<HashMap<UserAddress, Vec<UserPosition>>> = join_all(tasks)
        .await
        .into_iter()
        .filter_map(|bucket| bucket.ok())
        .collect();
    let mut raw_results = HashMap::new();
    for result_bucket in aggregate_results {
        raw_results.extend(result_bucket);
    }
    Ok(raw_results)
}

fn generate_user_by_position_by_asset(positions_by_user: HashMap<UserAddress, Vec<UserPosition>>) -> HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>> {
    let mut user_by_position_by_asset: HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>> = HashMap::new();
    for (user_address, positions) in positions_by_user.iter() {
        for position in positions {
            // if the asset already exists, get it. Otherwise create an empty map for it
            let users_by_position = user_by_position_by_asset.entry(position.underlying_asset.clone()).or_insert_with(HashMap::new);

            if position.scaled_variable_debt > U256::ZERO {
                // if the user has debt for that asset, add it to the borrowed vector or create a new empty one and then add it
                let users_vector = users_by_position.entry(PositionType::BORROWED).or_insert_with(Vec::new);
                users_vector.push(user_address.clone());
            }

            if position.usage_as_collateral_enabled_on_user && position.scaled_atoken_balance > U256::ZERO {
                // if the user has balance for that asset (and can be used as collateral), add it to the collateral vector or create a new empty one and then add it
                let users_vector = users_by_position.entry(PositionType::COLLATERAL).or_insert_with(Vec::new);
                users_vector.push(user_address.clone());
            }
        }
    }
    user_by_position_by_asset
}
