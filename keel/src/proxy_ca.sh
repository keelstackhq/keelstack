#!/usr/bin/env bash
# keel: trust the agent proxy's CA so git, npm, cargo and pip verify TLS behind it.
# The setup script starts in the parent of the checkouts: KEEL_REPO_DIR (set by keel just before
# this prelude) names the repo checkout; fall back to the cwd. Never fails the setup script.
keel_orig="$PWD"
if [ -n "${KEEL_REPO_DIR:-}" ] && [ -d "$KEEL_REPO_DIR" ]; then
  cd -- "$KEEL_REPO_DIR" 2>/dev/null || true
fi
hook=0
if [ -f ./scripts/proxy-ca.sh ]; then
  hook=1
  bash ./scripts/proxy-ca.sh install || true
fi
cd -- "$keel_orig" 2>/dev/null || true
ca=""
if [ -n "${PROXY_CA_BUNDLE:-}" ] && [ -f "$PROXY_CA_BUNDLE" ]; then
  ca="$PROXY_CA_BUNDLE"
elif [ -f /root/.ccr/ca-bundle.crt ]; then
  ca=/root/.ccr/ca-bundle.crt
fi
if [ -n "$ca" ]; then
  if [ "$hook" = 0 ]; then
    # No repo hook (hull's script absent): persist the CA variables ourselves.
    {
      echo "export NODE_EXTRA_CA_CERTS=\"$ca\""
      echo "export SSL_CERT_FILE=\"$ca\""
      echo "export REQUESTS_CA_BUNDLE=\"$ca\""
      echo "export CARGO_HTTP_CAINFO=\"$ca\""
      echo "export GIT_SSL_CAINFO=\"$ca\""
    } > "${KEEL_PROFILE_DIR:-/etc/profile.d}/keel-proxy-ca.sh" 2>/dev/null || true
    npm config set cafile "$ca" 2>/dev/null || true
  fi
  # The rest of the setup script (the bosun snippet) runs in this shell.
  export NODE_EXTRA_CA_CERTS="$ca" SSL_CERT_FILE="$ca" REQUESTS_CA_BUNDLE="$ca" CARGO_HTTP_CAINFO="$ca" GIT_SSL_CAINFO="$ca"
fi
true
