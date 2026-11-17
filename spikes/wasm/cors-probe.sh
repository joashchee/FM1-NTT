#!/bin/bash
# Phase 0 spike (docs/FM1TASTIK.md): can a browser on another origin fetch this URL? Follows redirects by hand and prints, per hop, the status and
# Access-Control-Allow-Origin. A browser's CORS fetch needs that header on EVERY hop, so one missing hop means blocked. curl is not a browser,
# but the header is what decides, and it is what this reads. Usage: spikes/wasm/cors-probe.sh URL [URL...]
origin="https://fm1tastik.chordica.com"
for u in "$@"; do
  echo "$u"
  for hop in 1 2 3 4 5 6; do
    h=$(curl -sS -m 20 -o /dev/null -D - -H "Origin: $origin" "$u" 2>&1 | tr -d '\r')
    st=$(echo "$h" | grep -E '^HTTP/[0-9.]+ [0-9]{3}' | grep -v 'Connection Established' | tail -1)
    ac=$(echo "$h" | grep -i -m1 '^access-control-allow-origin' || true)
    loc=$(echo "$h" | grep -i -m1 '^location:' | cut -d' ' -f2- || true)
    echo "  hop$hop  ${st:-no response}  |  ${ac:-no access-control-allow-origin}"
    [ -z "$loc" ] && break
    case "$loc" in http*) u="$loc" ;; *) u="$(echo "$u" | sed -E 's#(https?://[^/]+).*#\1#')$loc" ;; esac
  done
done
