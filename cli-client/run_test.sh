#!/usr/bin/env bash
echo "on stdout"
>&2 echo "on stderr"
[ -t 0 ] && echo "stdin is tty" || echo "stdin is not tty"
sleep 30
echo after

