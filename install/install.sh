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
#   EHAI_REQUIRE_SIGNATURE=1  fail instead of falling back to checksum-only
#                 when no minisign verifier can be found or fetched
set -eu

REPO="jagadeeshm007/ehai"
NAME="${EHAI_NAME:-ehai}"
CHANNEL="${EHAI_CHANNEL:-alpha}"
API="https://api.github.com/repos/$REPO"

say() { printf '%s\n' "$*" >&2; }
die() { say "error: $*"; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found"; }
need curl

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

# ---- verify -------------------------------------------------------------------
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  else die "need 'sha256sum' or 'shasum' to verify the download"; fi
}

# No minisign on the machine? Fetch the official static build into $TMP (deleted
# on exit, nothing installed system-wide). Hashes are pinned and were checked
# against the minisign author's key RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3.
MS_VER="0.12"
fetch_minisign() {
  case "$OS" in
    Linux)  f="minisign-$MS_VER-linux.tar.gz"; sum="9a599b48ba6eb7b1e80f12f36b94ceca7c00b7a5173c95c3efc88d9822957e73"; bin="minisign-linux/$TARGET_ARCH/minisign" ;;
    Darwin) f="minisign-$MS_VER-macos.zip";    sum="89000b19535765f9cffc65a65d64a820f433ef6db8020667f7570e06bf6aac63"; bin="minisign" ;;
  esac
  curl -fsSL -o "$f" "https://github.com/jedisct1/minisign/releases/download/$MS_VER/$f" || return 1
  if [ "$(sha256 "$f")" != "$sum" ]; then say "warning: downloaded minisign failed its pinned checksum, not using it"; return 1; fi
  case "$f" in
    *.zip) unzip -qo "$f" "$bin" >/dev/null 2>&1 || return 1 ;;
    *)     tar -xzf "$f" "$bin" 2>/dev/null || return 1 ;;
  esac
  chmod +x "$bin" && MINISIGN="$TMP/$bin"
}

MINISIGN="$(command -v minisign 2>/dev/null || true)"
if [ -z "$MINISIGN" ]; then
  say "minisign not installed; fetching a temporary copy to verify the signature..."
  fetch_minisign || MINISIGN=""
fi

if [ -n "$MINISIGN" ]; then
  say "Verifying signature..."
  curl_auth -o SHA256SUMS.minisig "$BASE_URL/SHA256SUMS.minisig"
  "$MINISIGN" -Vm SHA256SUMS -x SHA256SUMS.minisig -P RWTKHW5dGCGJOywz9PXOM27FdVwe+Eqj9qrgNrFh0faYS9LKS0kL6uhC >/dev/null \
    || die "signature verification failed — refusing to install"
elif [ "${EHAI_REQUIRE_SIGNATURE:-0}" = 1 ]; then
  die "could not get minisign and EHAI_REQUIRE_SIGNATURE=1 (brew install minisign, or apt install minisign)"
else
  say "warning: could not get minisign, skipping signature check (SHA-256 checksum is still verified)."
fi

say "Verifying checksum..."

EXPECTED="$(grep " $ASSET\$" SHA256SUMS | awk '{print $1}')"
[ -n "$EXPECTED" ] || die "$ASSET not listed in SHA256SUMS"
ACTUAL="$(sha256 "$ASSET")"
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
# Under `curl | sh` stdin is the script pipe, so the wizard would silently take
# every default. Reattach it to the terminal when there is one.
if [ -t 1 ] && (exec </dev/tty) 2>/dev/null; then
  "$INSTALLED" init </dev/tty
else
  "$INSTALLED" init
fi

say "Done. Open a new terminal (or source your shell rc) to use '$NAME'."
