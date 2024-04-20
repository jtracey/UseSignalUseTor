#!/bin/env bash

echo "This file is untested, and is intended for documentation purposes only."
exit 1

### UNTESTED!!! ###

## Execute from inside the MagiskOnEmulator directory
IMAGE_PATH=~/Android/Sdk/system-images/android-30/google_apis/x86/
if [ ! -f "$IMAGE_PATH/ramdisk.bak.img" ] ; then
    cp "$IMAGE_PATH/ramdisk.img" "$IMAGE_PATH/ramdisk.bak.img"
fi
cp "$IMAGE_PATH/ramdisk.bak.img" ramdisk.img
$EMULATOR_EXECUTABLE -avd $DEVICE_NAME -noaudio -camera-back none -qemu -enable-kvm &
./patch.sh
cp ramdisk.img "$IMAGE_PATH/ramdisk.img"
pkill $EMULATOR_EXECUTABLE
