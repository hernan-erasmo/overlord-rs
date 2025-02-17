use alloy::sol;

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

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    UniswapV3Quoter,
    "src/abis/uniswap_v3_quoter.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    UniswapV3Factory,
    "src/abis/uniswap_v3_factory.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    UniswapV3Pool,
    "src/abis/uniswap_v3_pool.json"
);

pub mod pool {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        AaveV3Pool,
        "src/abis/aave_v3_pool.json"
    );
}

pub type GetReserveConfigurationDataReturn = AaveProtocolDataProvider::getReserveConfigurationDataReturn;
