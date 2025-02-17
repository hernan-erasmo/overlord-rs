use super::cache::ProviderCache;
use super::constants::*;
use super::sol_bindings::{
    AaveProtocolDataProvider, AaveUIPoolDataProvider, GetReserveConfigurationDataReturn,
};
use alloy::primitives::{address, Address, U256};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ReserveConfigurationEnhancedData {
    pub symbol: String,
    pub data: GetReserveConfigurationDataReturn,
    pub liquidation_fee: U256,
}

pub type ReserveConfigurationData = HashMap<Address, ReserveConfigurationEnhancedData>;

pub async fn generate_reserve_details_by_asset(
    provider_cache: Arc<ProviderCache>,
) -> Result<ReserveConfigurationData, Box<dyn std::error::Error>> {
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
                }
                Err(e) => {
                    return Err(format!("Failed to get reserves list to initialize reserve configuration struct: {}", e).into());
                }
            }
        }
        Err(e) => {
            return Err(format!("Failed to get the provider to query reserves list: {}", e).into())
        }
    }

    let mut configuration_data: ReserveConfigurationData = HashMap::new();
    match provider_cache.get_provider().await {
        Ok(provider) => {
            let aave_config = AaveProtocolDataProvider::new(
                AAVE_V3_PROTOCOL_DATA_PROVIDER_ADDRESS,
                provider.clone(),
            );
            let unknown_asset = String::from("unknown_asset");
            for reserve_address in reserves {
                let symbol = symbols_by_address
                    .get(&reserve_address)
                    .unwrap_or(&unknown_asset);
                let data: GetReserveConfigurationDataReturn;
                let liquidation_fee: U256;
                match aave_config
                    .getReserveConfigurationData(reserve_address)
                    .call()
                    .await
                {
                    Ok(reserve_config) => data = reserve_config,
                    Err(e) => {
                        return Err(format!(
                            "Failed to get reserve configuration data for asset {}: {}",
                            reserve_address, e
                        )
                        .into())
                    }
                }
                match aave_config
                    .getLiquidationProtocolFee(reserve_address)
                    .call()
                    .await
                {
                    Ok(fee_response) => {
                        liquidation_fee = fee_response._0;
                    }
                    Err(e) => {
                        return Err(format!(
                            "Failed to get reserve liquidation fee for asset {}: {}",
                            reserve_address, e
                        )
                        .into())
                    }
                }
                configuration_data.insert(
                    reserve_address,
                    ReserveConfigurationEnhancedData {
                        symbol: symbol.clone(),
                        data,
                        liquidation_fee,
                    },
                );
            }
        }
        Err(e) => {
            return Err(format!(
                "Failed to get provider to query reserve configuration: {}",
                e
            )
            .into())
        }
    }
    Ok(configuration_data)
}
