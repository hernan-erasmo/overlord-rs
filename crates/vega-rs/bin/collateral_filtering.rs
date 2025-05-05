use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
};
use overlord_shared::{
    common::get_reserves_data,
    sol_bindings::{
        pool::AaveV3Pool, AaveOracle, AaveProtocolDataProvider, AaveUIPoolDataProvider,
        GetReserveConfigurationDataReturn, IERC20Metadata,
        IUiPoolDataProviderV3::AggregatedReserveData, ERC20,
    },
};
use std::{collections::HashMap, f64, sync::Arc};

pub const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address =
    address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");
const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
pub const AAVE_ORACLE_ADDRESS: Address = address!("0x54586bE62E3c3580375aE3723C145253060Ca0C2");

type ReserveAddress = Address;

pub const AAVE_V3_POOL_ADDRESS: Address = address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2");
pub const AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS: Address =
    address!("41393e5e337606dc3821075Af65AeE84D7688CBD");

#[derive(Debug, Clone)]
pub struct ReserveConfigurationEnhancedData {
    pub symbol: String,
    pub data: GetReserveConfigurationDataReturn,
    pub liquidation_fee: U256,
}

pub type ReserveConfigurationData = HashMap<Address, ReserveConfigurationEnhancedData>;

/// Get the symbol of a token, or return a default string in case of failure
async fn get_token_symbol(
    provider: Arc<RootProvider<PubSubFrontend>>,
    token_address: Address,
) -> String {
    let token = IERC20Metadata::new(token_address, provider.clone());
    match token.symbol().call().await {
        Ok(symbol) => symbol._0,
        Err(_) => "UNK_OR_UNDEF_SYMBOL".to_string(),
    }
}

/// Fetches information on aave reserves and returns a map of reserve addresses to their configuration data, symbol and liquidation fee.
pub async fn generate_reserve_details_by_asset(
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<ReserveConfigurationData, Box<dyn std::error::Error>> {
    // Get reserve addresses from AAVE getReservesList
    let reserve_addresses = match AaveV3Pool::new(AAVE_V3_POOL_ADDRESS, provider.clone())
        .getReservesList()
        .call()
        .await
    {
        Ok(reserves) => reserves._0,
        Err(e) => {
            return Err(format!(
                "Failed to get reserves list to initialize reserve configuration struct: {}",
                e
            )
            .into());
        }
    };
    let mut configuration_data: ReserveConfigurationData = HashMap::new();

    let aave_config =
        AaveProtocolDataProvider::new(AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, provider.clone());
    for reserve_address in reserve_addresses {
        let symbol = get_token_symbol(provider.clone(), reserve_address).await;
        let data = match aave_config
            .getReserveConfigurationData(reserve_address)
            .call()
            .await
        {
            Ok(reserve_config) => reserve_config,
            Err(e) => {
                return Err(format!(
                    "Failed to get reserve configuration data for asset {}: {}",
                    reserve_address, e
                )
                .into())
            }
        };
        let liquidation_fee = match aave_config
            .getLiquidationProtocolFee(reserve_address)
            .call()
            .await
        {
            Ok(fee_response) => fee_response._0,
            Err(e) => {
                return Err(format!(
                    "Failed to get reserve liquidation fee for asset {}: {}",
                    reserve_address, e
                )
                .into())
            }
        };
        configuration_data.insert(
            reserve_address,
            ReserveConfigurationEnhancedData {
                symbol: symbol.clone(),
                data,
                liquidation_fee,
            },
        );
    }
    Ok(configuration_data)
}

#[derive(Debug)]
pub struct UserPosition {
    pub scaled_atoken_balance: U256,
    pub usage_as_collateral_enabled_on_user: bool,
    pub scaled_variable_debt: U256,
    pub underlying_asset: ReserveAddress,
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

pub fn percent_div(value: U256, percentage: U256) -> U256 {
    ((value * U256::from(1e4)) + (percentage / U256::from(2))) / percentage
}

/// Remember collateral filtering is all about collateral, not debt
/// By the point this function is called and given a list of users, we already
/// know they have debt against the protocol, so only focus on collateral filtering
/// conditions.
pub async fn has_any_collateral_above_threshold(
    provider: Arc<RootProvider<PubSubFrontend>>,
    user_address: Address,
    user_positions: Vec<UserPosition>,
    min_collateral_in_usd: f64,
) -> Result<bool, Box<dyn std::error::Error>> {
    // This should be something that we query only once, and make available for other services via shared memory, IPC or whatever
    let reserves_data = get_reserves_data(provider.clone()).await?;
    let reserves_data = reserves_data
        .into_iter()
        .map(|d| (d.underlyingAsset, d))
        .collect::<HashMap<_, _>>();

    let collateral_positions = user_positions
        .into_iter()
        .filter(|p| p.scaled_atoken_balance > U256::ZERO && p.usage_as_collateral_enabled_on_user)
        .collect::<Vec<UserPosition>>();
    if collateral_positions.len() == 0 {
        println!("No (usable) collateral positions found");
        return Ok(false);
    }
    for position in collateral_positions {
        // get the aToken balance for the underlying asset
        let a_token = reserves_data
            .get(&position.underlying_asset)
            .unwrap()
            .aTokenAddress;
        let a_token_contract = ERC20::new(a_token, provider.clone());
        let a_token_balance = match a_token_contract.balanceOf(user_address).call().await {
            Ok(balance) => balance.balance,
            Err(e) => {
                eprintln!("Error trying to call balanceOf: {}", e);
                U256::ZERO
            }
        };

        // get the liquidation bonus for the underlying asset
        let liquidation_bonus = reserves_data
            .get(&position.underlying_asset)
            .unwrap()
            .reserveLiquidationBonus;

        // get the price of the underlying asset
        let price = get_asset_price(provider.clone(), position.underlying_asset).await;

        // get the decimals of the underlying asset
        let decimals = reserves_data
            .get(&position.underlying_asset)
            .unwrap()
            .decimals;

        let symbol = reserves_data
            .get(&position.underlying_asset)
            .unwrap()
            .symbol
            .clone();
        println!("Profit potential for {}:", symbol);

        let a_token_balance_in_asset_units =
            f64::from(a_token_balance) / f64::from(10).powi(decimals.try_into().unwrap_or(0));
        let raw = a_token_balance.as_limbs()[0] as f64; // Get the lowest limb which is u64, then convert to f64
        let token_units = raw / 10f64.powi(decimals.try_into().unwrap_or(0)); // normalize the token amount
        let a_token_balance_in_usd = token_units * (price.to::<u128>() as f64 / 1e8); // multiply by price, normalize 8 decimals
        println!(
            "\taToken balance = {}, in asset units = {}, in USD = ${}",
            a_token_balance, a_token_balance_in_asset_units, a_token_balance_in_usd,
        );

        // In normal operation, AAVE applies the liquidation bonus on top of the max available collateral to liquidate
        // for this filter, we apply it on top of the whole user collateral, assuming that if it's not above the profit
        // threshold, then it won't be above the profit threshold with max collateral to liquidate either, and thus discard the user
        let bonus_fraction = (f64::from(liquidation_bonus) - 10000.0) / 100.0;
        let bonus_in_usd = a_token_balance_in_usd * f64::from(bonus_fraction) / 100.0;
        println!(
            "\tLiquidation bonus in USD ({}% of ${}) = ${}",
            bonus_fraction, a_token_balance_in_usd, bonus_in_usd,
        );

        if bonus_in_usd >= min_collateral_in_usd {
            return Ok(true);
        };
    }
    Ok(false)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let user_address: Address = args[1].parse().expect("Invalid address format");

    let ipc = IpcConnect::new("/tmp/reth.ipc".to_string());
    let provider = ProviderBuilder::new().on_ipc(ipc).await?;
    let provider = Arc::new(provider);

    println!(
        "Calculating collateral threshold for address: {}",
        user_address
    );

    let mut user_positions: Vec<UserPosition> = vec![];
    let ui_data =
        AaveUIPoolDataProvider::new(AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, provider.clone());
    let result = ui_data
        .getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, user_address)
        .call()
        .await;
    match result {
        Ok(data) => {
            user_positions = data
                ._0
                .iter()
                .map(|d| UserPosition {
                    scaled_atoken_balance: d.scaledATokenBalance,
                    usage_as_collateral_enabled_on_user: d.usageAsCollateralEnabledOnUser,
                    scaled_variable_debt: d.scaledVariableDebt,
                    underlying_asset: d.underlyingAsset,
                })
                .collect();
        }
        Err(e) => {
            println!("Couldn't calculate address reserves: {:?}", e);
            return Err(e.into());
        }
    }

    let min_collateral_in_usd = 1.5 as f64;
    let verdict = match has_any_collateral_above_threshold(
        provider,
        user_address,
        user_positions,
        min_collateral_in_usd,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            return Err(format!(
                "Error calculating has_any_collateral_above_threshold: {}",
                e
            )
            .into())
        }
    };
    if verdict {
        println!("The user should've been included in the cache");
    } else {
        println!("The user didn't have any collateral above threshold. DO NOT INLCUDE.");
    }
    Ok(())
}
