#!/bin/bash

rm -rf shadow.data/
shadow --pcap-enabled true --template-directory shadow.data.template shadow.yaml > shadow.log ;
ret=$?

declare -A groups
groups[group1]="Alice Bob Carol"
groups[group2]="Alice Bob Dave"

check_one_log() {
    counts="$(grep -Ec "$1" shadow.data/hosts/client*/*.stdout | cut -d: -f2 | sort -n)"
    if [[ $(echo "$counts" | tail -1) -lt $2 ]] ; then
        echo "Not enough matches of pattern: $1"
        ret=1
    fi
    if [[ $(echo "$counts" | tail -2 | head -1) -gt 0 ]] ; then
        echo "Found matches of pattern in multiple logs: $1"
        ret=1
    fi
}

check_all_logs() {
    counts="$(grep -Ec "$1" shadow.data/hosts/client*/*.stdout | cut -d: -f2 | sort -n)"
    if [[ $(echo "$counts" | tail -1) -lt $2 ]] ; then
        echo "Not enough matches of pattern: $1"
        ret=1
    fi
}

invert_check_all_logs() {
    if grep -Eq "$1" shadow.data/hosts/client*/*.stdout ; then
        echo "Found errors via pattern: $1"
        ret=1
    fi
}

for group in ${!groups[@]} ; do
    for name in ${groups[$group]} ; do
        echo "$group,$name"
        # user sent at least 10 messages to the group
        check_one_log "$name,$group,send," 10
        # user got at least 10 receipts from the group
        check_one_log "$name,$group,receive,.*,receipt" 10
        # users got at least 10 receipts from this user in this group
        check_all_logs "$group,receive,.*,$name,receipt" 10
        # user got at least 10 normal messages from the group
        check_one_log "$name,$group,receive,.*,[0-9]+" 10
        # users got at least 10 normal messages from this user in this group
        check_all_logs "$group,receive,.*,$name,[0-9]+" 10
        # users didn't get any errors
        invert_check_all_logs "[Ee]rr"
    done
done

if [ $ret == 0 ] ; then
    echo "All tests passed."
else
    echo "At least one test failed."
    exit $ret
fi
