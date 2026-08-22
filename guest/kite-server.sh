#!/bin/sh
# kite-server.sh v5 — kitewright MCP watchdog (self-detaching via setsid).
# No reverse tunnels: inbound arrives over the guest NIC (100.96.0.2:8090) via
# smolvm's published-TCP forwarder (machine -p 8090:8090). MCP_HTTP_BIND must
# be 0.0.0.0 for that to work (127.0.0.1-only binds are invisible to -p).
# v5: depth-2 health check — a live-process-but-broken kite (HTTP no answer)
# is killed after 2 consecutive failures, the same way a dead one is respawned.
# Launched with: machine exec --name kite -- sh /root/kite-server.sh
# Idempotent: if this watchdog loop is already alive (pid file + kill -0),
# a second launch exits immediately — no double watchdogs, no double kite.
if test -f /tmp/.kite-loop-pid && kill -0 $(cat /tmp/.kite-loop-pid) 2>/dev/null; then
  exit 0
fi
setsid sh -c '
  : > /tmp/.kite-watchdog.log
  echo "$$" > /tmp/.kite-loop-pid
  fails=0
  while true; do
    P=""; test -f /tmp/.kite-pid && P=$(cat /tmp/.kite-pid 2>/dev/null)
    if test -z "$P" || ! kill -0 "$P" 2>/dev/null; then
      export BROWSER_EXECUTABLE=/usr/bin/chromium
      export BROWSER_NO_SANDBOX=1
      export KITE_HEADLESS=1
      export MCP_HTTP_BIND=0.0.0.0:8090
      setsid /usr/local/bin/kite >/tmp/kite.log 2>&1 &
      echo $! > /tmp/.kite-pid
      fails=0
      echo "$(date) kite launched pid $!" >> /tmp/.kite-watchdog.log
    elif curl -s -m 2 -o /dev/null http://127.0.0.1:8090/ 2>/dev/null; then
      # Any HTTP answer counts as alive (kite returns 404 on / — the real
      # endpoint is /mcp; `curl -f` would misjudge that as broken and kill
      # the healthy process in a restart loop).
      fails=0
    else
      fails=$((fails+1))
      if test $fails -ge 2; then
        echo "$(date) kite unhealthy ($fails), killing pid $P" >> /tmp/.kite-watchdog.log
        kill -9 "$P" 2>/dev/null
        : > /tmp/.kite-pid
        fails=0
      fi
    fi
    sleep 3
  done
' </dev/null >/dev/null 2>&1 &
echo detached
