#!/bin/sh
# cdp-server.sh v3 — CDP chromium watchdog + socat bridge (self-detaching).
# Chromium ignores --remote-debugging-address (binds loopback only), so socat
# bridges the published-port path: 100.96.0.2:9223 -> 127.0.0.1:9223.
# Inbound arrives via machine -p 9222:9223 (published TCP -> NIC address).
# v3: depth-2 health check — chromium with a live process but dead DevTools
# (e.g. the known "broken network stack" instance) is killed after 2
# consecutive probe failures, same as a dead one.
# Launched with: machine exec --name kite -- sh /root/cdp-server.sh
# Idempotent: if this watchdog loop is already alive (pid file + kill -0),
# a second launch exits immediately — no double watchdogs, no double
# chromium/socat.
if test -f /tmp/.cdp-loop-pid && kill -0 $(cat /tmp/.cdp-loop-pid) 2>/dev/null; then
  exit 0
fi
setsid sh -c '
  : > /tmp/.cdp-watchdog.log
  echo "$$" > /tmp/.cdp-loop-pid
  (
    fails=0
    while true; do
      P=""; test -f /tmp/.cdp-chromium.pid && P=$(cat /tmp/.cdp-chromium.pid 2>/dev/null)
      if test -z "$P" || ! kill -0 "$P" 2>/dev/null; then
        /usr/bin/chromium --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage --no-first-run --remote-debugging-port=9223 about:blank >/tmp/cdp-chromium.log 2>&1 &
        echo $! > /tmp/.cdp-chromium.pid
        fails=0
        echo "$(date) chromium launched pid $!" >> /tmp/.cdp-watchdog.log
      elif curl -s -m 2 -o /dev/null http://127.0.0.1:9223/json/version 2>/dev/null; then
        fails=0
      else
        fails=$((fails+1))
        if test $fails -ge 2; then
          echo "$(date) chromium unhealthy ($fails), killing pid $P" >> /tmp/.cdp-watchdog.log
          kill -9 "$P" 2>/dev/null
          : > /tmp/.cdp-chromium.pid
          fails=0
        fi
      fi
      sleep 3
    done
  ) &
  (
    while true; do
      socat TCP4-LISTEN:9223,bind=100.96.0.2,reuseaddr,fork TCP4:127.0.0.1:9223
      echo "$(date) socat exited, restarting" >> /tmp/.cdp-watchdog.log
      sleep 0.5
    done
  ) &
  wait
' </dev/null >/dev/null 2>&1 &
echo detached
