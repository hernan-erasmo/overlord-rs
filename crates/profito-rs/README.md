# profito-rs

Given an underwater user alert (that is, HF < 1), `profito-rs` aims to answer the question of _is this user profitable to liquidate?_

## Calculation

The most basic sanity check we can make is net profitability, which is composed of 3 parts

$NetProfit = LiquidationBonus - DeterministicCosts - NonDeterministicCosts$

### Liquidation Bonus

`Liquidation Bonus` is what _adds_ to the equation.

$LiquidationBonus = DebtRepaid × BonusMultiplier − DebtRepaid$

### Deterministic Costs

`Deterministic Costs` are the ones we either know for sure how much they're going to eat into our profits, or that we can at least bound somehow.

$DeterministicCosts = (GasUsed × BaseFee) + (DebtRepaid × FlashLoanRate) + (CollateralReceived × SlippageRate)$

### NonDeterministic Costs

`NonDeterministic costs` are the ones that we can't easily predict, or that we can't easily know how much we're going to need to make our TX land.

$NonDeterministicCosts$ = $(GasUsed × PriorityFee)+ CoinbaseBribe − RefundedETH$

## Open questions

### 1. How do we determine which debt to repay when liquidating?
TBA

### 2. Why would we ever want to ask for a refund ETH amount? (only saw this on beaver, btw)
TBA

### 3. What exactly is `slippage rate`? Do we even need to account for it?
TBA

### 4. If `CLOSE_FACTOR_HF_THRESHOLD` < HF < 1, then only 50% of the debt can be liquidated. If HF < `CLOSE_FACTOR_HF_THRESHOLD`, then 100% of the debt can be liquidated. Is `CLOSE_FACTOR_HF_THRESHOLD` an attribute of the reserve?

No, `CLOSE_FACTOR_HF_THRESHOLD` is hardcoded into the [LiquidationLogic](https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L68C27-L68C63) contract, and it's defined as 0.95e18. If the HF is below 1, but above that, then only 50% can be liquidated. If it's under that value, then you can liquidate 100% of whatever debt you choose.
