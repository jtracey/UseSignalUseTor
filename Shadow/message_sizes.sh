#!/bin/env sh

if [ $# -lt 1 ] ; then
    echo "usage: $0 msgstore.db"
    exit 1
fi

sizes=$(sqlite3 "$1" 'SELECT file_length FROM message_media WHERE file_length != 0;' | sort -n)
lines=$(
    for size in $sizes; do
        if [ $size -le 112 ] ; then
            echo 1
        else
            echo $(( ($size + 208)/160 ))
        fi
    done | uniq -c | sed 's/^\s*//')

(echo $(echo "$lines" | cut -d' ' -f1); echo $(echo "$lines" | cut -d' ' -f2)) | sed 's/ /,/g'
