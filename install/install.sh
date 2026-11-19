#!/usr/bin/env sh
# ehai installer (macOS, Linux). See install.ps1 for Windows.
#
# Public repo (no token needed):
#   curl -fsSL https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.sh | sh
#
# Private repo (org deployment) — provide a GitHub PAT with `contents: read`:
#   EHAI_TOKEN=ghp_xxx sh <(curl -fsSL -H "Authorization: Bearer $EHAI_TOKEN" \
#     "https://raw.githubusercontent.com/YOUR_ORG/ehai/main/install/install.sh")
#
# After install the token is saved to ~/.ehai/token (mode 0600) so `ehai update`
# works without re-entering it. Clear it with: rm ~/.ehai/token
#
# Env overrides:
#   EHAI_TOKEN    GitHub PAT (private repo) — highest priority auth source
#   EHAI_NAME     install under a different command name (default: ehai)
#   EHAI_CHANNEL  alpha | beta | stable (default: alpha)
#   GITHUB_TOKEN  fallback if EHAI_TOKEN and `gh auth token` are absent
set -eu

REPO="jagadeeshm007/ehai"
NAME="${EHAI_NAME:-ehai}"
CHANNEL="${EHAI_CHANNEL:-alpha}"
API="https://api.github.com/repos/$REPO"

say() { printf '%s\n' "$*" >&2; }
die() { say "error: $*"; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found"; }
need curl
command -v minisign >/dev/null 2>&1 \
  || die "minisign is required to verify the release signature (brew install minisign, or apt install minisign)"

# ---- auth -------------------------------------------------------------------
# Resolution order: EHAI_TOKEN → GITHUB_TOKEN/GH_TOKEN → gh auth token
TOKEN=""
if [ -n "${EHAI_TOKEN:-}" ]; then
  TOKEN="$EHAI_TOKEN"
elif [ -n "${GITHUB_TOKEN:-}" ]; then
  TOKEN="$GITHUB_TOKEN"
elif [ -n "${GH_TOKEN:-}" ]; then
  TOKEN="$GH_TOKEN"
elif command -v gh >/dev/null 2>&1; then
  TOKEN="$(gh auth token 2>/dev/null || true)"
fi

# --location-trusted forwards auth headers through GitHub's CDN redirect,
# which is required for private-repo release asset downloads. GitHub's CDN
# (objects.githubusercontent.com) is still GitHub/Microsoft infrastructure,
# so forwarding the token there is safe and expected for private repos.
curl_auth() {
  if [ -n "$TOKEN" ]; then
    curl -fsSL --location-trusted -H "Authorization: Bearer $TOKEN" "$@"
  else
    curl -fsSL "$@"
  fi
}

# ---- platform target ---------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
  Linux)  TARGET_OS="unknown-linux-musl" ;;
  Darwin) TARGET_OS="apple-darwin" ;;
  *)      die "unsupported OS: $OS" ;;
esac
case "$ARCH" in
  x86_64|amd64)   TARGET_ARCH="x86_64" ;;
  arm64|aarch64)  TARGET_ARCH="aarch64" ;;
  *)              die "unsupported arch: $ARCH" ;;
esac
TARGET="${TARGET_ARCH}-${TARGET_OS}"

# ---- pick a release for the channel -----------------------------------------
say "Looking up the latest $CHANNEL release..."
RELEASES_JSON="$(curl_auth "$API/releases")"

TAG=""
for t in $(printf '%s' "$RELEASES_JSON" | grep -o '"tag_name": *"[^"]*"' | sed 's/.*"\([^"]*\)"$/\1/'); do
  case "$CHANNEL" in
    alpha)  TAG="$t" ;;
    beta)   case "$t" in *-alpha*) continue ;; *) TAG="$t" ;; esac ;;
    stable) case "$t" in *-*) continue ;; *) TAG="$t" ;; esac ;;
    *)      die "unknown channel: $CHANNEL" ;;
  esac
  [ -n "$TAG" ] && break
done
[ -n "$TAG" ] || die "no $CHANNEL release found for $REPO"
say "Installing $TAG for $TARGET"

ASSET="ehai-${TAG#v}-${TARGET}.tar.gz"
BASE_URL="https://github.com/$REPO/releases/download/$TAG"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cd "$TMP"

curl_auth -o "$ASSET" "$BASE_URL/$ASSET"
curl_auth -o SHA256SUMS "$BASE_URL/SHA256SUMS"
curl_auth -o SHA256SUMS.minisig "$BASE_URL/SHA256SUMS.minisig"

# ---- verify -------------------------------------------------------------------
say "Verifying signature and checksum..."
minisign -Vm SHA256SUMS -x SHA256SUMS.minisig -P RWTKHW5dGCGJOywz9PXOM27FdVwe+Eqj9qrgNrFh0faYS9LKS0kL6uhC \
  || die "signature verification failed — refusing to install"

EXPECTED="$(grep " $ASSET\$" SHA256SUMS | awk '{print $1}')"
[ -n "$EXPECTED" ] || die "$ASSET not listed in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$ASSET" | awk '{print $1}')"
else
  ACTUAL="$(shasum -a 256 "$ASSET" | awk '{print $1}')"
fi
[ "$EXPECTED" = "$ACTUAL" ] || die "checksum mismatch for $ASSET"

# ---- install ------------------------------------------------------------------
tar -xzf "$ASSET"
DIR="ehai-${TAG#v}-${TARGET}"
chmod +x "$DIR/ehai"

if [ "$NAME" != "ehai" ]; then
  mv "$DIR/ehai" "$DIR/$NAME"
fi
STAGE_BIN="$TMP/$DIR/$NAME"

say "Running '$NAME self install'..."
"$STAGE_BIN" self install

INSTALLED="$HOME/.ehai/bin/$NAME"

# Persist the token so `ehai update` works without re-entering it
if [ -n "$TOKEN" ]; then
  TOKEN_FILE="$HOME/.ehai/token"
  printf '%s' "$TOKEN" > "$TOKEN_FILE"
  chmod 0600 "$TOKEN_FILE"
  say "Token saved to $TOKEN_FILE for future updates."
fi

say "Running '$NAME init'..."
"$INSTALLED" init

say "Done. Open a new terminal (or source your shell rc) to use '$NAME'."
