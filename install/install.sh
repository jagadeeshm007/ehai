#!/usr/bin/env sh
# ehai installer (macOS, Linux). See install.ps1 for Windows.
#
#   curl -fsSL https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.sh | sh
#
# The repo is public, so no auth is required. `gh auth token` / `GITHUB_TOKEN`
# are used when present only to avoid unauthenticated GitHub API rate limits.
#
# Env overrides:
#   EHAI_NAME     install under a different command name (default: ehai)
#   EHAI_CHANNEL  alpha | beta | stable (default: alpha)
#   GITHUB_TOKEN  used if `gh auth token` isn't available
set -eu

REPO="jagadeeshm007/ehai"
NAME="${EHAI_NAME:-ehai}"
CHANNEL="${EHAI_CHANNEL:-alpha}"
API="https://api.github.com/repos/$REPO"

say() { printf '%s\n' "$*" >&2; }
die() {
  say "error: $*"
  exit 1
}

need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found"; }
need curl
command -v minisign >/dev/null 2>&1 \
  || die "minisign is required to verify the release signature (brew install minisign, or apt install minisign)"

# ---- auth -------------------------------------------------------------------
TOKEN=""
if command -v gh >/dev/null 2>&1; then
  TOKEN="$(gh auth token 2>/dev/null || true)"
fi
if [ -z "$TOKEN" ] && [ -n "${GITHUB_TOKEN:-}" ]; then
  TOKEN="$GITHUB_TOKEN"
fi
curl_auth() {
  if [ -n "$TOKEN" ]; then
    curl -fsSL -H "Authorization: Bearer $TOKEN" "$@"
  else
    curl -fsSL "$@"
  fi
}

# ---- platform target ---------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
Linux) TARGET_OS="unknown-linux-musl" ;;
Darwin) TARGET_OS="apple-darwin" ;;
*) die "unsupported OS: $OS" ;;
esac
case "$ARCH" in
x86_64 | amd64) TARGET_ARCH="x86_64" ;;
arm64 | aarch64) TARGET_ARCH="aarch64" ;;
*) die "unsupported arch: $ARCH" ;;
esac
TARGET="${TARGET_ARCH}-${TARGET_OS}"

# ---- pick a release for the channel -----------------------------------------
say "Looking up the latest $CHANNEL release..."
RELEASES_JSON="$(curl_auth "$API/releases")"

# GitHub lists releases newest-first, so the first tag the channel accepts is
# the latest one for that channel — no version sorting needed. The release
# workflow never creates drafts, so there's nothing to filter out here.
# alpha accepts every tag; beta skips -alpha tags; stable wants no suffix at all.
TAG=""
for t in $(printf '%s' "$RELEASES_JSON" | grep -o '"tag_name": *"[^"]*"' | sed 's/.*"\([^"]*\)"$/\1/'); do
  case "$CHANNEL" in
  alpha) TAG="$t" ;;
  beta) case "$t" in *-alpha*) continue ;; *) TAG="$t" ;; esac ;;
  stable) case "$t" in *-*) continue ;; *) TAG="$t" ;; esac ;;
  *) die "unknown channel: $CHANNEL" ;;
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
say "Running '$NAME init'..."
"$INSTALLED" init

say "Done. Open a new terminal (or source your shell rc) to use '$NAME'."
