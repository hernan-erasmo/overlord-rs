# profito-rs

Given an underwater user alert (that is, HF < 1), `profito-rs` aims to answer the question of _is this user profitable to liquidate?_

## Calculation

The most basic sanity check we can make is net profitability, which is composed of 3 parts

$Net\ Profit = Liquidation\ Bonus - Deterministic\ Costs - Non$-$deterministic\ Costs$

### Liquidation Bonus

`Liquidation Bonus` is what _adds_ to the equation.

$Liquidation\ Bonus = Debt\ Repaid × Bonus\ Multiplier − Debt\ Repaid$

### Deterministic Costs

`Deterministic Costs` are the ones we either know for sure how much they're going to eat into our profits, or that we can at least bound somehow.

$Deterministic\ Costs = (Gas\ Used × Base\ Fee) + (Debt\ Repaid × Flash\ Loan\ Rate) + (Collateral\ Received × Slippage\ Rate)$

### Non-deterministic Costs

`Non-deterministic costs` are the ones that we can't easily predict, or that we can't easily know how much we're going to need to make our TX land.

$Non$-$deterministic\ Costs$ = $(Gas\ Used × Priority\ Fee)+ Coinbase\ Bribe − Refunded\ ETH$

## Open questions

1. How do we determine which debt to repay when liquidating?
2. Why would we ever want to ask for a refund ETH amount? (only saw this on beaver, btw)
3. What exactly is `slippage rate`? Do we even need to account for it?
