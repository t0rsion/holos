#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
BATS_INCLUDE=${BATS_INCLUDE:-$ROOT/local/external/TDA_Updating_Persistence/BATS/include}
CXX=${CXX:-g++}
OUTPUT=${OUTPUT:-$ROOT/target/v10-external/bats-warm}

if [ ! -f "$BATS_INCLUDE/bats.hpp" ]; then
    echo "BATS_INCLUDE does not contain bats.hpp: $BATS_INCLUDE" >&2
    exit 1
fi

mkdir -p "$(dirname -- "$OUTPUT")"
exec "$CXX" -std=c++17 -O3 -fopenmp -Wall -Wextra -Wpedantic \
    -I"$BATS_INCLUDE" -I"$ROOT/benchmarks/v10_external" \
    "$ROOT/benchmarks/v10_external/bats_warm.cpp" \
    "$ROOT/benchmarks/v10_external/trajectory.cpp" \
    -o "$OUTPUT"
