use crate::calculations::BestPair;

use overlord_shared::{
    constants::{
        AAVE_V3_POOL_ADDRESS, AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS, AAVE_V3_PROVIDER_ADDRESS,
        AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
    },
    sol_bindings::{
        pool::AaveV3Pool, AaveProtocolDataProvider, AaveUIPoolDataProvider,
        GetReserveConfigurationDataReturn, IERC20Metadata, IUiPoolDataProviderV3::UserReserveData,
    },
};

use alloy::primitives::{aliases::U24, Address, U256};
use alloy::providers::RootProvider;

use alloy::pubsub::PubSubFrontend;
use ethers_core::{
    abi::{encode, Token},
    types::{
        transaction::eip2718::TypedTransaction, Eip1559TransactionRequest, H160, U256 as ethersU256,
    },
    utils::keccak256,
};
use std::{collections::HashMap, env, sync::Arc};

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

/// Get's the list of user reserves, but only returns those that the user has at least some debt or collateral and,
/// for the later, the ones that are allowed to be used as collateral
pub async fn get_user_reserves_data(
    provider: Arc<RootProvider<PubSubFrontend>>,
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

pub async fn create_trigger_liquidation_tx(
    best: BestPair,
    user_address: Address,
    collateral_to_weth_fee: U24,
    weth_to_debt_fee: U24,
    bribe: U256,
) -> Result<TypedTransaction, Box<dyn std::error::Error>> {
    let params = vec![Token::Tuple(vec![
        Token::Uint(ethersU256::from_little_endian(
            &best.actual_debt_to_liquidate.to_le_bytes::<32>(),
        )), // debtAmount
        Token::Address(H160::from_slice(user_address.as_slice())), // user
        Token::Address(H160::from_slice(best.debt_asset.as_slice())), // debtAsset
        Token::Address(H160::from_slice(best.collateral_asset.as_slice())), // collateral
        Token::Uint(ethersU256::from(collateral_to_weth_fee.to::<u32>())), // collateralToWethFee
        Token::Uint(ethersU256::from(weth_to_debt_fee.to::<u32>())), // wethToDebtFee
        Token::Uint(ethersU256::from(bribe.to::<u16>())),          // bribePercentBps
        Token::Uint(ethersU256::from(best.flash_loan_source as u8)), // flashLoanSource
        Token::Uint(ethersU256::from(0)),                          // aavePremium
    ])];

    let function_signature =
        "triggerLiquidation((uint256,address,address,address,uint24,uint24,uint16,uint8,uint256))";
    let selector = &keccak256(function_signature.as_bytes())[0..4];
    let encoded_params = encode(&params);
    let encoded = [selector, &encoded_params].concat();
    let foxdie_owner = match &env::var("FOXDIE_OWNER") {
        Ok(addr_str) => match addr_str.parse::<H160>() {
            Ok(addr) => addr,
            Err(e) => {
                return Err(format!(
                    "Couldn't convert FOXDIE_OWNER value into formal address: {}",
                    e
                )
                .into())
            }
        },
        Err(e) => {
            return Err(format!("Couldn't read FOXDIE_OWNER environment value: {}", e).into())
        }
    };
    let foxdie_address = match &env::var("FOXDIE_ADDRESS") {
        Ok(addr_str) => match addr_str.parse::<H160>() {
            Ok(addr) => addr,
            Err(e) => {
                return Err(format!(
                    "Couldn't convert FOXDIE_ADDRESS value into formal address: {}",
                    e
                )
                .into())
            }
        },
        Err(e) => {
            return Err(format!("Couldn't read FOXDIE_ADDRESS environment value: {}", e).into())
        }
    };
    let tx = Eip1559TransactionRequest::new()
        .from(foxdie_owner)
        .to(foxdie_address)
        .data(encoded.to_vec());
    Ok(TypedTransaction::Eip1559(tx))
}
