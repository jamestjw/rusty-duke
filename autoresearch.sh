#!/bin/bash
set -euo pipefail

cargo check --quiet
cargo run --release --quiet --example autoresearch_bench
