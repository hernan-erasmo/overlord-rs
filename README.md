# overlord-rs

## Instructions

Clone and build. Check `.env.example` for the list of environment variables to be set in `.env` for everything to work.

After that, take a look at the `startup.sh` script for different ways of running things. You probably want to use the following, but YMMV

```

```

## Playbooks

### We're missing liquidations because we don't see the triggering price update.

Means `oops` didn't see the update, plain and simple. Now, knowing _why_ is the complicated part.

First thing, make sure the block (or the block previous to the liquidation) had any `Forward` calls.

Also, make sure `oops` didn't output any parsing errors around the time of the price update tx. That could signal a change in the way price updates are submitted.

If nothing looks out of the ordinary, then we need to ask ourselves: Are we tracking the originator? `oops` works by listening to all pending tx's, filtering by those that come from specific addresses and then filtering again by those that call the `transmit()` function (wrapped in calldata from a `forward()` call). If the sender of the price update tx is not in our tracked list, we need to consider updating the addresses file. For that, you need to follow these steps:

1. From the `nodebuster` repo, make sure the virtual environment is on (otherwise run `source ./bin/activate`)
2. Run `python ./src/main.py --force`. This is the main script that will parse all our data sources and pull new information. The `--force` flag means it ignores whatever was cached on the previous run.
3. Cross your fingers that nothing happens but, if it does, then just read the error messages and make the required changes.

The algorithm for getting from reserves to oracles relies heavily on constantly-changing third-party data, so it frequently requires modifications in order to adapt to it.


## Debugging logs (at least until Datadog is set up)

Let's say you want to see events starting on 2025-01-30. Use the following command to get the line number of the first matching line

```
cat /var/log/overlord-rs/overlord-rs-processed.log | grep -n "2025-01-30" | head -n1 | cut -d: -f1
```

That should return the line number of the first line of 2025-01-30. Then use that number on the command below and you'll have all lines after that one in the `filtered.log` file.

```
tail -n +66000 /var/log/overlord-rs/overlord-rs-processed.log > filtered.log
```

And if you want to put an upper limit on the number of lines in the output, then use `head` like this:

```
tail -n +66000 /var/log/overlord-rs/overlord-rs-processed.log | head -n 1000 > filtered.log
```
