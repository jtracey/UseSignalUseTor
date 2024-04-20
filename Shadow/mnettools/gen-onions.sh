#!/bin/bash

hosts_dir="$1"
for i in {0..10}; do
    for ClientDir in "$hosts_dir"/*client*/; do
        if [ -f $ClientDir/$i.torrc ] ; then
            DataDirectory=$ClientDir/tor-$i
            chmod 700 $ClientDir/*.tor
            chmod 700 $DataDirectory
            tor --hush -f $ClientDir/$i.torrc --ControlPort 0 --DisableNetwork 1 --DataDirectory $DataDirectory &
        fi
    done
    echo "terminating set $i"
    pkill -P $$
    wait
done

echo "replacing hosts file"
for ClientDir in "$hosts_dir"/*client*/; do
    for UserDir in $ClientDir/user*.tor; do
        if [ -d "$UserDir" ] ; then
            user=$(basename "$UserDir" .tor)
            onion=$(cat "$UserDir"/hostname)
            sed -i "s/^$user:/$onion:/g" "$hosts_dir/hosts"
        fi
    done
done
