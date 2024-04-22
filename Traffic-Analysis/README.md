This requires quite a few pieces to get working properly.
 - The Android sdk. This can be obtained either with Android Studio, or the standalone CLI tools.
   - Download either from the [Android Studio site](https://developer.android.com/studio) (command line tools are near the bottom).
 - The following Android sdk components, which can be installed either via Android Studio's SDK Manager or the `sdkmanager` CLI tool:
   - The official Android emulator. You'll specifically need an x86 or x86-64 image without gapps (gAPIs is fine, and in some apps, necessary). Tested: Google APIs, API 30, x86, r10
     - Make sure when downloading APKs, you get the matching platform or "universal" version.
   - adb (Android Debug Bridge)
   - the platform for the android version being used
   - In Android Studio, the emulator itself and adb come with the default install; system images can be found in the SDK Manager
   - See the [`sdkmanager` docs](https://developer.android.com/tools/sdkmanager) for installing all the above with the CLI (adb is in platform-tools)
 - [Magisk APK](https://github.com/topjohnwu/Magisk/releases). Used to get an accessible `su` binary on the emulator, and to provide a framework for allowing some additional mucking around with the installed apps. Tested: v25.2
 - MagiskOnEmulator. A small set of tools used to get Magisk working on an emulator. Tested: Custom revision ([`./MagiskOnEmulator/`](MagiskOnEmulator/))
 - [Termux APK](https://github.com/termux/termux-app/releases). Used to get a package manager and more fully-featured shell environment on Android. Tested: `termux-app_v0.118.0+github-debug_x86.apk`
 - SSH. Used to access Termux from the host shell.
 - sshd, tsu, tcpdump, mitmproxy packages on Termux. Used to allow ssh, sudo, packet captures, and MITM'ing traffic in android. (These are installed as part of the instructions below.)
 - [frida](https://frida.re/docs/installation/): Used to modify executables. Tested: 16.0.19
 - [magisk-frida](https://github.com/ViRb3/magisk-frida/releases): Used to allow Frida to modify running apps via Magisk. Tested: 16.0.19-1
 - The APK for the app you'll be MITM'ing. Because root is not (easily) available on emulators with the Play Store, you'll likely need to find some source to download it from. [Signal provides APKs for download](https://signal.org/android/apk/).
   - A good source for other APKs is the [EFF's apkeep tool](https://github.com/EFForg/apkeep)

## Why do we capture traffic on the emulator instead of from the host?
A few reasons:
 - It makes it simpler to ensure that all traffic being captured originates from the particular emulator. While there are ways to filter traffic to just emulator traffic from the host, they're not as simple as one might expect, and being able to just capture everything is a nice feature to have, especially when running multiple emulators at the same time.
 - Getting the same setup working on a real device instead of an emulator is nearly a subset of the steps to getting it working on the emulator. If we were capturing packets from a real device on the host, we'd need another set of instructions to do that. As is, the only extra step needed on a real device is setting up SSH access to the device.
 - Similarly, if using a real device, this setup allows traffic to be recorded without a host machine at all, meaning traffic can be recorded on real devices moving between WiFi and cellular networks.

That said, if you wish to do live captures from Wireshark with working Signal filters, simply skip the tcpdump steps below, use sshfs to make the keylog file available to the host, set Wireshark's TLS options to use the file from sshfs, and capture all traffic on the desired emulator (Wireshark only checks for TLS keys when it first sees the handshake, so you need something like sshfs to ensure the keyfile gets updated before the handshake completes).

## Set up the emulator
 - Create an x86 or x86-64 image without gapps (gAPIs is fine, and for WhatsApp, necessary).
   - e.g., `avdmanager create avd -n Messenger1 -k "system-images;android-30;default;x86_64" -c 10G`
 - Copy the directory `<sdk_home>/system-images/<platform>/<arch>/` to `<sdk_home>/system-images/<platform>/<arch>_root/` (i.e., add `_root` or similar to the copy's name; this copy is important because the following steps can be temperamental, and because updating the images can otherwise break the device and delete its files).
 - In `~/.android/avd/<name>/config.ini` (or whichever path you configured the device to be located), change the `image.sysdir.1` field to point to the new copy.
   - You may also want to ensure `hw.keyboard=yes` is set.
 - Copy the `<sdk_home>/system-images/<platform>/<arch>_root/ramdisk.img` file into the MagiskOnEmulator folder.
 - Copy the Magisk APK to the MagiskOnEmulator folder as `magisk.apk`.
 - Start the emulator.
   - If using the CLI, this will be using the `emulator` executable in the emulator directory of your SDK directory.
 - Run `patch.sh` from the MagiskOnEmulator directory.
 - Copy `ramdisk.img` back to the respective system-image directory, overwriting the original file.
 - Power off the emulator.
 - Cold start the emulator. (**This must be a cold start!** If using Android Studio, do not use the power button on the emulator, use the extended controls; if using the CLI, add `-no-snapshot-load` to your emulator command.)
 - Confirm that the emulator successfully booted, and that the Magisk app reports Magisk as being installed (not just that the app was installed, but Magisk itself).
 - Move the magisk-frida zip file onto the device (e.g., `adb push MagiskFrida.zip /sdcard`).
 - In the Magisk app, go to the Modules tab, and install the MagiskFrida file from where you pushed it (n.b.: despite the name, files pushed to `/sdcard` go into the main system storage, not the virtual sdcard).
 - Press the reboot button it presents on successful install.
 - Install termux (`adb install <termux apk>`).
 - Inside termux, install the root-repo, upgrade, then binutils, python, python-pip, rust, openssh, screen, tsu, frida, and tcpdump:
   - `pkg install root-repo && pkg upgrade -y && pkg install binutils python python-pip rust openssh screen tsu frida tcpdump -y`
 - Inside termux, install mitmproxy with pip (`pip install mitmproxy`).
 - Inside termux, run `termux-setup-storage` and accept the Android prompt to give it permission to access shared files.
 - Inside termux, run `sudo echo` and accept the Magisk prompt to give it root access.
 - Add an ssh public key to the authorized keys in termux:
   - `adb root && adb shell "echo '$(cat ~/.ssh/*.pub)' >> /data/data/com.termux/files/home/.ssh/authorized_keys"`
 - Install the app you are going to MITM, if you have not already done so.
 - Power off.

## MITM'ing android app traffic
 - Optional: run `source completions.bash` to give the current bash shell tab completions for `mitm-emulator.sh`.
 - Read the help from `mitm-emulator.sh help`.
 - Set the environment variables described in the last step as appropriate and run:
   - `mitm-emulator.sh emulator`
   - `mitm-emulator.sh mitm`
   - `mitm-emulator.sh tcpdump`
   - `mitm-emulator.sh app`
 - Perform whatever actions you wish to MITM (if you'd like to see the mitmproxy interface for live information on captures, run `mitm-emulator.sh attach-mitmproxy`).
 - Run:
   - `mitm-emulator.sh stop-mitm`
   - `mitm-emulator.sh pull`
   - `mitm-emulator.sh stop-emulator`

## View the MITM'd data with Wireshark
 - Ensure that reassembly is fully enabled:
   - Edit -> Preferences -> Protocols -> TCP (n.b.: you can type the name to jump to the protocol)
   - Ensure "Allow subdissector to reassemble TCP streams" and "Reassemble out-of-order segments" are enabled.
 - Enable TLS decryption:
   - Edit -> Preferences -> Protocols -> TLS
   - Ensure all "Reassemble..." options are enabled, then select the pulled key file in "(Pre)-Master-Secret log filename".
 - Open the pcap file.
 - Filter to `(http or websocket) and ip.addr != 127.0.0.1` to see the most relevant application-layer data.
   - To view accurate timing and order information about packets instead, you'll need to disable the TCP and TLS reassemble options mentioned above, which will render much of the application data inaccessible.
 - To parse the protobufs in Signal messages:
   - Edit -> Preferences -> Protocols -> ProtoBuf
   - Edit the search paths to include the [`protobufs`](protobufs/) directory of this project, and ensure "Dissect Protobuf fields as Wireshark fields" and "Show details of message, fields and enums" are enabled.
   - Copy the `signal_protobuf.lua` file to your [Wireshark plugin folder](https://www.wireshark.org/docs/wsug_html_chunked/ChPluginFolders.html).
   - Restart Wireshark, open the pcap file again, and enable the filter.
   - Go to Analyze -> Enabled Protocols, and enable the `signal_protobuf`, `signal_body`, and `signal_content` protocols.
   - Note that this will attempt to parse all protobufs in websocket connections as Signal messages sent between clients, including protobufs from other applications or to the Signal server. In these mistaken cases, the protobuf will generally fail to parse correctly (unlike self-describing formats like JSON, protobufs are difficult or impossible to parse without knowing the data structure). Fields displayed from protobufs that aren't actually Signal messages between clients should not be interpreted as correct or meaningful.
