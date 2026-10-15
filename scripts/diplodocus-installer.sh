#!/bin/sh
set -eu

# Download completely before running so a failed transfer cannot execute a partial script.
installer_dir=$(mktemp -d "${TMPDIR:-/tmp}/diplodocus-installer.XXXXXX")
trap 'rm -rf "$installer_dir"' 0
trap 'exit 1' HUP INT TERM

curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/jolars/diplodocus/releases/latest/download/diplodocus-installer.sh \
  -o "$installer_dir/install.sh"
sh "$installer_dir/install.sh" "$@"
