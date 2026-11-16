#!/bin/sh
# Membuka Lumen dalam mode privat: cookie dan data situs tidak ditulis ke disk.
exec "$(dirname "$0")/lumen" --private "$@"
