#!/bin/bash

if [[ $# -lt 2 ]] ; then
    echo "usage: $0 stats_dir_in stats_dir_out"
    exit 1
fi

stats_dir_in="$1"
stats_dir_out="$2"

n_files=$(ls "$stats_dir_in" | wc -l)
N=$(( $n_files / $(nproc) ))

ls "$stats_dir_in" | while mapfile -n $N files_per_proc && [ ${#files_per_proc[@]} -gt 0 ]; do
        files="$(printf "$stats_dir_in/%s" "${files_per_proc[@]}")"
        python3 get_w.py "$stats_dir_out" $files &
done
wait

echo "all done"
