#!/bin/bash

set -euo pipefail

NUM_NETWORKS="${NUM_NETWORKS:-10}"
SCALE="${SCALE:-0.1}"
#USER_COUNT="${USER_COUNT:-10000}"
PROJECT_ROOT="${PROJECT_ROOT:-.}"

echo "generating $NUM_NETWORKS at scale $SCALE from $PROJECT_ROOT"


source "$PROJECT_ROOT/tornettools/toolsenv/bin/activate"
export PATH="$PROJECT_ROOT/tor/src/core/or:$PROJECT_ROOT/tor/src/app:$PROJECT_ROOT/tor/src/tools:${PATH}"

for i in `seq 0 $((NUM_NETWORKS - 1))`; do
  # this part can't be done in parallel :/
  echo "$i: running tornettools..."
  # generate tor network
  tornettools generate \
    relayinfo_staging_2023-04-01--2023-04-30.json \
    userinfo_staging_2023-04-01--2023-04-30.json \
    networkinfo_staging.gml \
    tmodel-ccs2018.github.io \
    --network_scale "$SCALE" \
    --prefix "tornet-$SCALE-$i"
done

echo "All done!"
