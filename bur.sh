#!/bin/bash
cargo build --release --workspace > /dev/null 2>&1 && ./target/release/bpchecker "$1"
