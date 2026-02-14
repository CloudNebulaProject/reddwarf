#!/usr/bin/bash
#
# SMF method script for svc:/system/reddwarf:default
#
# Reads configuration from SMF properties and launches
# the reddwarf agent daemon.
#

. /lib/svc/share/smf_include.sh

REDDWARF_BIN="/opt/reddwarf/bin/reddwarf"

getprop() {
    svcprop -p "application/$1" "$SMF_FMRI" 2>/dev/null
}

case "$1" in
start)
    node_name=$(getprop node_name)
    listen_addr=$(getprop listen_addr)
    data_dir=$(getprop data_dir)
    storage_pool=$(getprop storage_pool)
    pod_cidr=$(getprop pod_cidr)
    etherstub_name=$(getprop etherstub_name)
    tls_enabled=$(getprop tls_enabled)
    tls_cert=$(getprop tls_cert)
    tls_key=$(getprop tls_key)

    # Default node_name to hostname if not set
    if [ -z "$node_name" ]; then
        node_name=$(hostname)
    fi

    # Ensure data directory parent exists
    data_parent=$(dirname "$data_dir")
    mkdir -p "$data_parent"

    # Build command line
    cmd="$REDDWARF_BIN agent"
    cmd="$cmd --node-name $node_name"
    cmd="$cmd --bind $listen_addr"
    cmd="$cmd --data-dir $data_dir"
    cmd="$cmd --storage-pool $storage_pool"
    cmd="$cmd --pod-cidr $pod_cidr"
    cmd="$cmd --etherstub-name $etherstub_name"

    if [ "$tls_enabled" = "true" ]; then
        cmd="$cmd --tls"
        if [ -n "$tls_cert" ] && [ -n "$tls_key" ]; then
            cmd="$cmd --tls-cert $tls_cert"
            cmd="$cmd --tls-key $tls_key"
        fi
    fi

    # Launch the daemon
    exec $cmd &

    exit $SMF_EXIT_OK
    ;;

*)
    echo "Usage: $0 { start }"
    exit $SMF_EXIT_ERR_FATAL
    ;;
esac
