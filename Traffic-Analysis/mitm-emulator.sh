#!/usr/bin/env bash

EMULATOR_EXEC="${EMULATOR_EXEC:-$HOME/Android/Sdk/emulator/emulator}"
DEVICE_NAME="${DEVICE_NAME:-Pixel_2_API_30}"
CONSOLE_PORT="${CONSOLE_PORT:-5554}"
export ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-$CONSOLE_PORT}"
SSH_PORT="${SSH_PORT:-$(( $CONSOLE_PORT + 10000 ))}"
APP_NAME="${APP_NAME:-org.thoughtcrime.securesms}"
TCPDUMP_FILE="${TCPDUMP_FILE:-/sdcard/android.pcap}"
SSLKEYLOGFILE="${SSLKEYLOGFILE:-/sdcard/keyfile.txt}"
COLLATE_KEYS="${COLLATE_KEYS:-true}"
MITMPROXY_SCREEN="${MITMPROXY_SCREEN:-mitmproxy}"
FRIDA_SCREEN="${FRIDA_SCREEN:-frida}"
TCPDUMP_SCREEN="${TCPDUMP_SCREEN:-tcpdump}"

LAUNCH_PROBE='/data/data/com.termux/files/home/launch.probe'

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

function launch_emulator {
    $EMULATOR_EXEC -avd $DEVICE_NAME -port $CONSOLE_PORT -noaudio -camera-back none -qemu -enable-kvm &
}

function push_scripts {
    adb push manage-screen.sh '/data/data/com.termux/files/home/.'
}

function hit_enter {
    adb shell input keyevent 66
}

function hit_tab {
    adb shell input keyevent 61
}

function hit_back {
    adb shell input keyevent 4
}

function enable_airplane_mode {
    adb shell settings put global airplane_mode_on 1
    adb shell am broadcast -a android.intent.action.AIRPLANE_MODE
}

function disable_airplane_mode {
    adb shell settings put global airplane_mode_on 0
    adb shell am broadcast -a android.intent.action.AIRPLANE_MODE
}

function get_app_user {
    app=$1
    local app_user="$(adb shell dumpsys package $app | grep 'userId=' | cut -d= -f2 | sort -u)"
    if [[ -z "$app_user" ]]; then
        echo "Couldn't find $app install, aborting">&2
        exit 1
    fi
    echo "$app_user"
}

function launch_termux {
    echo "launching termux"
    adb root >/dev/null
    while ! adb shell 'am start -W -n com.termux/.HomeActivity' ; do
        # I really wish there were a better way to wait for the OS to finish booting
        # (`am` fails if you try to use it too soon after sys.boot_completed)
        echo "failed to launch termux, trying again in 10 seconds..."
        sleep 10
    done
    # the emulator can sometimes be a little slow to launch the app
    while ! adb shell "ls $LAUNCH_PROBE" 2>/dev/null; do
        echo "waiting for launch.probe"
        sleep 5
        adb shell input text "touch\ $LAUNCH_PROBE" && hit_enter
    done
    echo "found launch.probe"
    adb shell "rm $LAUNCH_PROBE" && echo "removed launch.probe"
}

function launch_sshd {
    adb wait-for-device shell 'while [[ -z $(getprop sys.boot_completed) ]]; do sleep 1; done'
    # Android really has no way to tell when it's booted enough to launch apps
    sleep 60
    launch_termux
    adb shell input text 'sshd' && hit_enter
    sleep 5
    echo forwarding $SSH_PORT to 8022
    adb forward tcp:$SSH_PORT tcp:8022
    echo forwarded
}

function run_termux_command {
    local termux_user="$(get_app_user com.termux)"
    if ! ssh -p $SSH_PORT $termux_user@localhost "$1" ; then
        echo "ssh failed to run command '$1'"
        echo "if ssh is complaining about host identification, you're running the emulator locally, and are okay with trusting the extra accepted localhost keys, you can fix this by running:"
        echo "ssh-keyscan -p $SSH_PORT localhost | sort -u >> ~/.ssh/known_hosts"
        echo "(you may need to restart the emulator after for this tool to work correctly)"
        exit 1
    fi
}

function apply_iptables {
    local app_uid=$(get_app_user $1)
    local command="LD_PRELOAD='' sudo iptables -t nat -A OUTPUT -p tcp -m owner --uid-owner $app_uid -j DNAT --to 127.0.0.1:8080 --dport "
    run_termux_command "$command 80"
    run_termux_command "$command 443"
}

function launch_mitmproxy {
    run_termux_command "./manage-screen.sh $MITMPROXY_SCREEN && screen -S $MITMPROXY_SCREEN -X stuff 'SSLKEYLOGFILE=\"$SSLKEYLOGFILE\" mitmproxy --mode transparent -k^M'"
}

function attach_mitmproxy {
    local termux_user="$(get_app_user com.termux)"
    ssh -p $SSH_PORT $termux_user@localhost -t "screen -rdS '$MITMPROXY_SCREEN'"
}

function launch_mitmd_app {
    local termux_home='/data/data/com.termux/files/home/'
    local cert_dst='/data/local/tmp/cert-der.crt'
    run_termux_command \
        "sudo cp $termux_home/.mitmproxy/mitmproxy-ca-cert.cer $cert_dst"
    run_termux_command "sudo chmod o+r $cert_dst"
    local user=$(get_app_user $APP_NAME)
    enable_airplane_mode
    adb shell am force-stop $APP_NAME
    adb shell pm disable $APP_NAME
    adb shell pm compile -f -m space $APP_NAME
    adb shell pm enable $APP_NAME
    adb shell monkey -p $APP_NAME -c android.intent.category.LAUNCHER 1
    # N.B.: Frida made a breaking change, where <v16 required --no-pause, whereas >= 16 that's the default, and adding it causes an unrecognized argument error
    screen -dmS "$FRIDA_SCREEN" frida -D $ANDROID_SERIAL --load "${SCRIPT_DIR}/frida-android-unpinning/frida-script.js" -f $APP_NAME
    disable_airplane_mode
}

function launch_tcpdump {
    run_termux_command "./manage-screen.sh $TCPDUMP_SCREEN && screen -S $TCPDUMP_SCREEN -X stuff 'sudo tcpdump -i any -w $TCPDUMP_FILE^M'"
}

function pull_keyfile {
    keyfile="$(basename "$SSLKEYLOGFILE")"
    if [ "$COLLATE_KEYS" == true ] && [ -f "$keyfile" ] ; then
        pullfile="$(mktemp)"
        sortfile="$(mktemp)"
        adb pull "$SSLKEYLOGFILE" "$pullfile"
        sort -u "$keyfile" "$pullfile" > "$sortfile"
        mv "$sortfile" "$keyfile"
        rm "$pullfile"
    else
        adb pull "$SSLKEYLOGFILE"
    fi
}

function print_help {
    echo "USAGE: $0 <COMMAND> [OPTIONS]"
    echo "where <COMMAND> is one of:"
    echo "emulator           run the emulator, and prepare it for other commands"
    echo "mitm               launch mitmproxy in a screen session on the emulator"
    echo "tcpdump            launch tcpdump in a screen session on the emulator"
    echo "app                launch app proxied through mitmproxy (disables then renables"
    echo "                   emulator network access and the app, and launches frida in a"
    echo "                   screen session on this machine)"
    echo "pull               pull the most recent keylogfile and pcap file,"
    echo "                   generated from mitmproxy and tcpdump respectively,"
    echo "                   to the current working directory"
    echo "stop-mitm          use pkill on the device to stop mitmproxy and tcpdump,"
    echo "                   and pkill on this machine to stop frida"
    echo "stop-emulator      shut down the emulated device"
    echo "attach-mitmproxy   attach this shell to the remote screen session mitmproxy is"
    echo "                   running in (use 'ctrl+a d' to detatch)"
    echo "ssh                ssh to the device"
    echo "help               print this help message"
    echo
    echo "The following environment variables are used to configure behavior"
    echo "(ordered roughly according to how likely you'll need to set it):"
    echo "EMULATOR_EXEC      path to the emulator executable"
    echo "                   (if this is unset but ANDROID_HOME is set,"
    echo "                   this script will use a path relative to that)"
    echo "                   current value: $EMULATOR_EXEC"
    echo "DEVICE_NAME        name of the emulator device to use"
    echo "                   current value: $DEVICE_NAME"
    echo "APP_NAME           name of the app being MITM'd"
    echo "                   current value: $APP_NAME"
    echo "CONSOLE_PORT       port on the host machine the emulator listens for console connections"
    echo "                   (this must be unique for each emulator running concurrently,"
    echo "                   and is expected by adb to be even)"
    echo "                   current value: $CONSOLE_PORT"
    echo "SSH_PORT           port on the host machine to forward SSH traffic through to the emulator"
    echo "                   (this must be unique for each emulator running concurrently;"
    echo "                   defaults to CONSOLE_PORT + 10000)"
    echo "                   current value: $SSH_PORT"
    echo "TCPDUMP_FILE       file on the emulator to store the packet capture"
    echo "                   (be sure to make this unique if capturing multiple emulators,"
    echo "                   else pulls will clobber each other)"
    echo "                   current value: $TCPDUMP_FILE"
    echo "SSLKEYLOGFILE      file on the emulator to store TLS keys"
    echo "                   current value: $SSLKEYLOGFILE"
    echo "COLLATE_KEYS       if true, pull adds new keys to the existing SSLKEYLOGFILE,"
    echo "                   else, pull overwites the local SSLKEYLOGFILE"
    echo "                   (n.b.: mitmproxy on the emulator will collate regardless)"
    echo "                   current value: $COLLATE_KEYS"
    echo "FRIDA_SCREEN       name of the local screen session to run frida on"
    echo "                   current value: $FRIDA_SCREEN"
    echo "MITMPROXY_SCREEN   name of the android screen session to run mitmproxy on"
    echo "                   current value: $MITMPROXY_SCREEN"
    echo "TCPDUMP_SCREEN     name of the android screen session to run tcpdump on"
    echo "                   current value: $TCPDUMP_SCREEN"
}

case "$1" in
    emulator)
        launch_emulator;
        launch_sshd;
        apply_iptables $APP_NAME ;;
    mitm)
        push_scripts;
        launch_mitmproxy ;;
    tcpdump)
        push_scripts;
        launch_tcpdump ;;
    app)
        launch_mitmd_app ;;
    pull)
        pull_keyfile;
        adb pull "$TCPDUMP_FILE" ;;
    stop-mitm)
        run_termux_command 'sudo pkill tcpdump';
        run_termux_command 'pkill mitmproxy';
        pkill frida ;;
    stop-emulator)
        adb shell reboot -p;
        # the emulator takes some time to shut down, and can get corrupted if
        # you try to launch before it finishes. 20 seconds is what the GUI uses.
        sleep 20 ;;
    attach-mitmproxy)
        attach_mitmproxy ;;
    ssh)
        termux_user="$(get_app_user com.termux)"
        ssh -p $SSH_PORT $termux_user@localhost ;;
    h|-h|help|--help)
        print_help ;;
    *)
        print_help; exit 1 ;;
esac
