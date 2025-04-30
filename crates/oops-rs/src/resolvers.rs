use std::sync::Arc;

use alloy::{primitives::{address, Address}, providers::RootProvider, pubsub::PubSubFrontend};
use once_cell::sync::Lazy;

use overlord_shared::sol_bindings::{sDAIAggregator::sDAISynchronicityPriceAdapter, CLSynchronicityPriceAdapterPegToBase, CbETHAggregator::CbETHPriceCapAdapter, EBTCAggregator::EBTCPriceCapAdapter, EthXAggregator::EthXPriceCapAdapter, OsETHAggregator::OsETHPriceCapAdapter, PriceCapAdapterStable, RETHAggregator::RETHPriceCapAdapter, RsETHAggregator::RsETHPriceCapAdapter, SUSDeAggregator::SUSDePriceCapAdapter, WeETHAggregator::WeETHPriceCapAdapter, WstETHAggregator::WstETHPriceCapAdapter};

use std::future::Future;
type ResolverFunction = fn(Arc<RootProvider<PubSubFrontend>>, Address) -> Box<dyn Future<Output = Address> + Send + 'static>;

/// These are living data structures, in the sense that will need to be updated if a new asset is onboarded
/// into AAVE v3.
/// If we don't have a resolver for that type, then a resolver function would need to be added as well,
/// so that it can return the address of the EACAggreagator at the root of the hierarchy.
pub static EAC_AGGREGATOR_PROXY_ORACLES: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("5f4eC3Df9cbd43714FE2740f5E3616155c5b8419"),
        address!("Cd627aA160A6fA45Eb793D19Ef54f5062F20f33f"),
        address!("ec1D1B3b0443256cc3860e24a46F108e699484Aa"),
        address!("DC3EA94CD0AC27d9A86C180091e7f78C683d3699"),
        address!("dF2917806E30300537aEB49A7663062F4d1F2b5F"),
        address!("553303d460EE0afB37EdFf9bE42922D8FF63220e"),
        address!("5C00128d4d1c2F4f652C267d7bcdD7aC99C16E16"),
        address!("c929ad75B72593967DE83E7F7Cda0493458261D9"),
        address!("4E155eD98aFE9034b7A5962f6C84c86d869daA9d"),
        address!("7A9f34a0Aa917D438e9b6E630067062B7F8f6f3d"),
        address!("f8fF43E991A81e6eC886a3D281A2C6cC19aE70Fc"),
        address!("6Ebc52C8C1089be9eB3945C4350B68B8E4C2233f"),
        address!("F4030086522a5bEEa4988F8cA5B36dbC97BeE88c"),
        address!("C7e9b623ed51F033b32AE7f1282b1AD62C28C183"),
        address!("F02C1e2A3B77c1cacC72f72B44f7d0a4c62e4a85"),
        address!("b41E773f507F7a7EA890b1afB7d2b660c30C8B0A"),
    ];
    oracles
});

pub static PRICE_CAP_ADAPTER_STABLE_ORACLES: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("736bF902680e68989886e9807CD7Db4B3E015d3C"),
        address!("4F01b76391A05d32B20FA2d05dD5963eE8db20E6"),
        address!("aEb897E1Dc6BbdceD3B9D551C71a8cf172F27AC4"),
        address!("C26D4a1c46d884cfF6dE9800B6aE7A8Cf48B4Ff8"),
        address!("45D270263BBee500CF8adcf2AbC0aC227097b036"),
        address!("02AeE5b225366302339748951E1a924617b8814F"),
        address!("150bAe7Ce224555D39AfdBc6Cb4B8204E594E022"),
        address!("9eCdfaCca946614cc32aF63F3DBe50959244F3af"),
        address!("f0eaC18E908B34770FDEe46d069c846bDa866759"),
    ];
    oracles
});

/// ie. wstETH, cbETH, rETH, osETH, ETHx, etc
pub static SPECIFIC_PRICE_CAP_ADAPTERS: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("6243d2F41b4ec944F731f647589E28d9745a2674"),
        address!("B4aB0c94159bc2d8C133946E7241368fc2F2a010"),
        address!("f112aF6F0A332B815fbEf3Ff932c057E570b62d3"),
        address!("5AE8365D0a30D67145f0c55A08760C250559dB64"),
        address!("0A2AF898cEc35197e6944D9E0F525C2626393442"),
        address!("D6270dAabFe4862306190298C2B48fed9e15C847"),
        address!("47F52B2e43D0386cF161e001835b03Ad49889e3b"),
        address!("95a85D0d2f3115702d813549a80040387738A430"),
    ];
    oracles
});

pub static CL_SYNCHRO_PRICE_PEG_ADAPTERS: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("230E0321Cf38F09e247e50Afc7801EA2351fe56F"),
        address!("b01e6C9af83879B8e06a092f0DD94309c0D497E4"),
    ];
    oracles
});

pub static SUSDE_PRICE_ADAPTERS: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("42bc86f2f08419280a99d8fbEa4672e7c30a86ec"),
    ];
    oracles
});

pub static SDAI_PRICE_ADAPTERS: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("29081f7aB5a644716EfcDC10D5c926c5fEe9F72B"),
    ];
    oracles
});

pub static GHO_ADAPTER: Lazy<Vec<Address>> = Lazy::new(|| {
    let oracles = vec![
        address!("D110cac5d8682A3b045D5524a9903E031d70FCCd"),
    ];
    oracles
});

/// Caller needs to check the return value and handle the special GHO case (because
/// that aggregator doesn't implement `getTransmitters()`)
pub async fn resolve_aggregator(
    provider: Arc<RootProvider<PubSubFrontend>>,
    oracle_address_for_aave: Address
) -> Result<Address, Box<dyn std::error::Error>> {
    match oracle_address_for_aave {
        addr if EAC_AGGREGATOR_PROXY_ORACLES.contains(&addr) => 
            Ok(resolve_eac_aggregator_proxy(provider, addr).await),
            
        addr if PRICE_CAP_ADAPTER_STABLE_ORACLES.contains(&addr) => 
            Ok(resolve_asset_to_usd_aggregator(provider, addr).await),
            
        addr if SPECIFIC_PRICE_CAP_ADAPTERS.contains(&addr) => 
            Ok(resolve_base_to_usd_aggregator(provider, addr).await),
            
        addr if CL_SYNCHRO_PRICE_PEG_ADAPTERS.contains(&addr) => 
            Ok(resolve_asset_to_peg(provider, addr).await),
            
        addr if SUSDE_PRICE_ADAPTERS.contains(&addr) => 
            Ok(resolve_susde_aggregator(provider, addr).await),
            
        addr if SDAI_PRICE_ADAPTERS.contains(&addr) => 
            Ok(resolve_dai_to_usd_aggregator(provider, addr).await),

        addr if GHO_ADAPTER.contains(&addr) =>
            Ok(resolve_gho_aggregator(provider, addr).await),

        _ => return Err(format!("This price oracle didn't match any group: {}", oracle_address_for_aave).into())
    }
}

pub async fn resolve_eac_aggregator_proxy(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    oracle_address_for_aave
}

pub async fn resolve_gho_aggregator(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    oracle_address_for_aave
}

pub async fn resolve_asset_to_usd_aggregator(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    match PriceCapAdapterStable::new(oracle_address_for_aave, _provider.clone()).ASSET_TO_USD_AGGREGATOR().call().await {
        Ok(response) => response._0,
        Err(_) => Address::ZERO,
    }
}

pub async fn resolve_asset_to_peg(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    match CLSynchronicityPriceAdapterPegToBase::new(oracle_address_for_aave, _provider.clone()).ASSET_TO_PEG().call().await {
        Ok(response) => response._0,
        Err(_) => Address::ZERO,        
    }
}

pub async fn resolve_susde_aggregator(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    match SUSDePriceCapAdapter::new(
        oracle_address_for_aave,
        _provider.clone())
    .BASE_TO_USD_AGGREGATOR().call().await {
        Ok(response) => response._0,
        Err(_) => Address::ZERO
    }
}
   
pub async fn resolve_dai_to_usd_aggregator(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    match sDAISynchronicityPriceAdapter::new(oracle_address_for_aave, _provider.clone()).DAI_TO_USD().call().await {
        Ok(response) => response._0,
        Err(_) => Address::ZERO,
    }
}

pub async fn resolve_base_to_usd_aggregator(_provider: Arc<RootProvider<PubSubFrontend>>, oracle_address_for_aave: Address) -> Address {
    match oracle_address_for_aave {
        addr if addr == address!("B4aB0c94159bc2d8C133946E7241368fc2F2a010") => {
            match WstETHPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("6243d2F41b4ec944F731f647589E28d9745a2674") => {
            match CbETHPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("5AE8365D0a30D67145f0c55A08760C250559dB64") => {
            match RETHPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("95a85D0d2f3115702d813549a80040387738A430") => {
            match EBTCPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("f112aF6F0A332B815fbEf3Ff932c057E570b62d3") => {
            match WeETHPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("0A2AF898cEc35197e6944D9E0F525C2626393442") => {
            match OsETHPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("D6270dAabFe4862306190298C2B48fed9e15C847") => {
            match EthXPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        addr if addr == address!("47F52B2e43D0386cF161e001835b03Ad49889e3b") => {
            match RsETHPriceCapAdapter::new(oracle_address_for_aave, _provider.clone()).BASE_TO_USD_AGGREGATOR().call().await {
                Ok(response) => response._0,
                Err(_) => Address::ZERO,
            }
        },
        _ => Address::ZERO,
    }
}
