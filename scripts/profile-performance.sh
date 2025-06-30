#!/bin/bash

# This script profiles a running process using perf and generates a flamegraph.
#
# Usage: ./profile-performance.sh <PID> <DURATION_SECONDS>
#
# Dependencies:
# - perf
# - flamegraph (https://github.com/brendangregg/FlameGraph)

set -e

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <PID> <DURATION_SECONDS>"
    exit 1
fi

PID=$1
DURATION=$2
OUTPUT_SVG="flamegraph_${PID}.svg"

echo "Profiling PID ${PID} for ${DURATION} seconds..."

# Run perf to record stack traces
perf record -F 99 -p ${PID} -g -- sleep ${DURATION}

echo "Generating flamegraph..."

# Process the perf data and generate the flamegraph
perf script | /usr/local/bin/FlameGraph/flamegraph.pl > ${OUTPUT_SVG}

echo "Flamegraph generated: ${OUTPUT_SVG}"