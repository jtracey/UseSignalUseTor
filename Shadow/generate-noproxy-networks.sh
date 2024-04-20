#!/bin/bash

set -euo pipefail

NUM_NETWORKS="${NUM_NETWORKS:-10}"
SCALE="${SCALE:-0.1}"
NUM_USERS="${NUM_USERS:-10000}"
PROJECT_ROOT="${PROJECT_ROOT:-.}"

echo "generating $NUM_NETWORKS at scale $SCALE with $NUM_USERS users from $PROJECT_ROOT"


source "$PROJECT_ROOT/tornettools/toolsenv/bin/activate"
export PATH="$PROJECT_ROOT/tor/src/core/or:$PROJECT_ROOT/tor/src/app:$PROJECT_ROOT/tor/src/tools:${PATH}"

# run mnettools with common arguments
run_mnettools () {
  local net_name="$1"
  local num_users="$2"
  local seed="$3"

  "$PROJECT_ROOT/mnettools/mnettools.py" \
    --dyadic "$PROJECT_ROOT/data/dyadic_count.dat" \
    --group "$PROJECT_ROOT/data/group-counts.dat" \
    --participants "$PROJECT_ROOT/data/group_sizes.no-individual.dat" \
    --config "$PROJECT_ROOT/tornettools/$net_name/shadow.config.yaml" \
    --clients "$PROJECT_ROOT/tornettools/$net_name/shadow.data.template/hosts/markovclient*/" \
    --empirical "$PROJECT_ROOT/data/stats/" \
    --users "$num_users" \
    --servers 10 \
    --tors 10 \
    --seed "$seed"
}

# main worker for generating the initial networks
generate_initial_networks () {
  local scale="$1"
  local net_number="$2"

  local vanilla_name="tornet-$scale-$net_number"
  local noproxy_name="$vanilla_name-mnet-noproxy-$NUM_USERS"

  echo "$net_number: copying to noproxy..."
  # leaving the original untouched in case we need to examine them later,
  # create a copy of the tor network for testing 10 thousand unproxied mgen users
  cp -r "$vanilla_name" "$noproxy_name"

  echo "$net_number: running mnettools..."
  # generate the mgen config
  run_mnettools "$noproxy_name" "$NUM_USERS" "$net_number"
  # mnettools doesn't overwrite the config
  mv "$noproxy_name"/mnet.shadow.config.yaml "$noproxy_name"/shadow.config.yaml

  echo "$net_number: done!"
}

for i in `seq 0 $((NUM_NETWORKS - 1))`; do
  generate_initial_networks "$SCALE" "$i" &
done
wait
sync

echo "All done!"
