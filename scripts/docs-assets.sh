#!/bin/sh
set -eu

output=${1:-site/docs}
cp scripts/diplodocus-installer.sh "$output/install"
cp scripts/diplodocus-installer.ps1 "$output/install.ps1"
cat > "$output/_headers" <<'HEADERS'
/install
  Content-Type: text/plain; charset=utf-8
  X-Content-Type-Options: nosniff
/install.ps1
  Content-Type: text/plain; charset=utf-8
  X-Content-Type-Options: nosniff
HEADERS
