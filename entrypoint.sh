#!/bin/bash
set -e

if [ "$DEBUG" = "true" ]; then
    echo "Starting in DEBUG mode with lldb-server..."
    exec lldb-server gdbserver *:1234 -- /usr/local/bin/vortex-relay-debug
else
    echo "Starting in NORMAL mode..."
    exec /usr/local/bin/vortex-relay
fi
