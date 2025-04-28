use alloy::sol;

sol!(
    #[allow(missing_docs)]
    function forward(
        address to,
        bytes calldata data
    ) external;

    #[allow(missing_docs)]
    function transmit(
        bytes32[3] calldata reportContext,
        bytes calldata report,
        bytes32[] calldata rs,
        bytes32[] calldata ss,
        bytes32 rawVs
    ) external override;

    #[allow(missing_docs)]
    function transmitSecondary(
        bytes32[3] calldata reportContext,
        bytes calldata report,
        bytes32[] calldata rs,
        bytes32[] calldata ss,
        bytes32 rawVs
    ) external override;

    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    contract ForwardToDestination {
        function transmitters() external view returns (address[] memory);
        function getTransmitters() external view returns (address[] memory);
    }
);
