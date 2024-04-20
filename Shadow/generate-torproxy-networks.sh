#!/bin/bash

set -euo pipefail

NUM_NETWORKS="${NUM_NETWORKS:-10}"
SCALE="${SCALE:-0.1}"
NUM_USERS="${NUM_USERS:-10000}"
PROJECT_ROOT="${PROJECT_ROOT:-.}"

echo "generating $NUM_NETWORKS at scale $SCALE with $NUM_USERS users from $PROJECT_ROOT"


source "$PROJECT_ROOT/tornettools/toolsenv/bin/activate"
export PATH="$PROJECT_ROOT/tor/src/core/or:$PROJECT_ROOT/tor/src/app:$PROJECT_ROOT/tor/src/tools:${PATH}"

# main worker for generating the initial networks
generate_torproxy_networks () {
  local scale="$1"
  local net_number="$2"

  local vanilla_name="tornet-$scale-$net_number"
  local noproxy_name="$vanilla_name-mnet-noproxy-$NUM_USERS"
  local torproxy_name="$vanilla_name-mnet-torproxy-$NUM_USERS"

  echo "$net_number: copying to torproxy..."
  # copy the unproxied version for the proxied mgen
  cp -r "$noproxy_name" "$torproxy_name"

  echo "$net_number: modying torproxy user configs..."
  # modify every user in the copy to use tor
  for d in "$torproxy_name"/shadow.data.template/hosts/markovclient* ; do
    sed -i 's/^#socks: "127.0.0.1:90/socks: "127.0.0.1:90/g' "$d"/user*.yaml &
  done

  echo "$net_number: done!"
}

for i in `seq 0 $((NUM_NETWORKS - 1))`; do
  generate_torproxy_networks "$SCALE" "$i" &
done
wait
sync

echo "All done!"
