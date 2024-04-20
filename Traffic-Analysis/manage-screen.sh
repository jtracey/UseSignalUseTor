#!/bin/sh
screen -wipe
## creates a detatched screen with name $1 iff it doesn't already exist
if ! screen -list 2>/dev/null | awk '{print $1}' | grep -q "$1$"; then
    screen -dmS $1
    echo "Created screen $1"
fi
