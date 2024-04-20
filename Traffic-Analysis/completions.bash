_mitm_completions() {
    local COMPLETES="emulator mitm tcpdump app pull stop-mitm stop-emulator attach-mitmproxy ssh help"
    COMPREPLY=( $(compgen -W "$COMPLETES" -- ${COMP_WORDS[COMP_CWORD]}) )
}

complete -F _mitm_completions mitm-emulator.sh
