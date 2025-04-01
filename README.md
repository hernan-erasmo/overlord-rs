# overlord-rs

## Instructions

Clone and build. Check `.env.example` for the list of environment variables to be set in `.env` for everything to work.

After that, take a look at the `startup.sh` script for different ways of running things. You probably want to use the following, but YMMV

```

```

## Playbooks

### We're missing liquidations because we don't see the triggering price update.

Most likely reason is that `oops` is not tracking the address that's calling `forward()` on the `EACAggregator` contract. Follow these steps:

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
