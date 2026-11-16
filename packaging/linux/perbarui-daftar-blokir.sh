#!/bin/sh
# Mengunduh daftar filter iklan/pelacak terbaru (EasyList, EasyPrivacy, ABPindo, uBlock)
# ke Data/filters. Tutup Lumen dulu; daftar dibaca saat browser dibuka.
set -u
base="$(cd "$(dirname "$0")" && pwd)"
dir="$base/Data/filters"
mkdir -p "$dir"
gagal=0
while read -r nama url; do
    case "$nama" in ""|\#*) continue ;; esac
    if curl -fsSL --retry 3 -o "$dir/$nama.tmp" "$url"; then
        mv "$dir/$nama.tmp" "$dir/$nama"
        echo "OK     $nama"
    else
        rm -f "$dir/$nama.tmp"
        echo "GAGAL  $nama (berkas lama tetap dipakai)" >&2
        gagal=1
    fi
done < "$base/filters.list"
exit $gagal
