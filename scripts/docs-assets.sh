#!/bin/sh
set -eu

output=${1:-site/docs}
for asset in favicon.svg favicon-96x96.png favicon.ico apple-touch-icon.png \
  web-app-manifest-192x192.png web-app-manifest-512x512.png site.webmanifest; do
  cp "assets/brand/$asset" "$output/$asset"
done
python3 - "$output" <<'PYTHON'
from pathlib import Path
import re
import sys

head = Path("scripts/docs-head.html").read_text().strip()
for page in Path(sys.argv[1]).rglob("*.html"):
    html = page.read_text()
    # Replace prior favicon declarations so repeated publishing stays idempotent.
    html = re.sub(
        r'<link\b(?=[^>]*\brel="(?:icon|shortcut icon|apple-touch-icon|manifest)")[^>]*>\s*'
        r'|<meta\b(?=[^>]*\bname="apple-mobile-web-app-title")[^>]*>\s*',
        "",
        html,
    )
    if "</head>" not in html:
        raise SystemExit(f"Missing </head> in {page}")
    page.write_text(html.replace("</head>", head + "\n</head>", 1))
PYTHON
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
