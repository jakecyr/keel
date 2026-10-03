#!/bin/sh
# Install published GitHub release assets. CI workflow artifacts are not releases.
set -eu

usage() {
    cat <<'HELP'
Install Keel from a published GitHub release:
  sh install.sh [--repo OWNER/REPO] [--version TAG] [--prefix DIRECTORY]
  sh install.sh [--repo OWNER/REPO] [--version TAG] [--bin-dir DIRECTORY]

  --repo       GitHub repository (default: jakecyr/keel).
  --version    Exact release tag (for example v0.1.0), or latest (default).
  --prefix     Install DIRECTORY/bin/keel (default: $HOME/.local).
  --bin-dir    Install DIRECTORY/keel; cannot be combined with --prefix.
  --help       Show this help without downloading anything.

Only HTTPS downloads are allowed. SHA-256 is checked before installing.
An existing regular binary is replaced atomically after successful validation.
Existing symlink or directory targets are refused. Shell profiles are unchanged.
Requires curl, tar, and sha256sum or shasum, on Linux/macOS x86_64/arm64.
The compiler additionally requires a C compiler (cc/clang/gcc) for native builds.

Expected release assets:
  keel-Linux-X64.tar.gz       keel-Linux-ARM64.tar.gz
  keel-macOS-X64.tar.gz       keel-macOS-ARM64.tar.gz
Each published platform asset needs a matching .tar.gz.sha256 file.
These assets must be attached to a GitHub Release before this installer works.
HELP
}
fail() { printf 'keel installer: %s\n' "$*" >&2; exit 1; }
need_value() { [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 requires a value"; }

keel_repo=jakecyr/keel
keel_version=latest
keel_prefix=
keel_bin_dir=
keel_seen_repo=0
keel_seen_version=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --help|-h) usage; exit 0 ;;
        --repo)
            need_value "$@"
            [ "$keel_seen_repo" -eq 0 ] || fail "--repo was specified twice"
            keel_repo=$2; keel_seen_repo=1; shift 2 ;;
        --version)
            need_value "$@"
            [ "$keel_seen_version" -eq 0 ] || fail "--version was specified twice"
            keel_version=$2; keel_seen_version=1; shift 2 ;;
        --prefix)
            need_value "$@"
            [ -z "$keel_prefix" ] && [ -z "$keel_bin_dir" ] || fail "choose one --prefix or --bin-dir"
            keel_prefix=$2; shift 2 ;;
        --bin-dir)
            need_value "$@"
            [ -z "$keel_prefix" ] && [ -z "$keel_bin_dir" ] || fail "choose one --prefix or --bin-dir"
            keel_bin_dir=$2; shift 2 ;;
        *) fail "unknown argument: $1 (see --help)" ;;
    esac
done

[ -n "$keel_repo" ] || fail "--repo OWNER/REPO must not be empty"
case "$keel_repo$keel_version" in
    *'
'*) fail "invalid --repo or --version: newline characters are not allowed" ;;
esac
printf '%s\n' "$keel_repo" | LC_ALL=C grep -Eq '^[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*$' ||
    fail "--repo must be OWNER/REPO using ordinary GitHub name characters"
printf '%s\n' "$keel_version" | LC_ALL=C grep -Eq '^(latest|v?[0-9]+(\.[0-9]+){1,2}([-+][A-Za-z0-9][A-Za-z0-9.+-]*)?)$' ||
    fail "--version must be latest or a release version such as v0.1.0"

if [ -z "$keel_bin_dir" ]; then
    if [ -z "$keel_prefix" ]; then
        [ -n "${HOME:-}" ] || fail "HOME is unset; supply --prefix or --bin-dir"
        keel_prefix=$HOME/.local
    fi
    keel_bin_dir=${keel_prefix%/}/bin
fi
case "$keel_bin_dir" in
    /*) ;;
    *) fail "installation directory must be an absolute path" ;;
esac
case "$keel_bin_dir" in
    /|*/../*|*/./*|*/..|*/.) fail "installation directory must not contain . or .. components" ;;
    *'
'*) fail "installation directory must not contain newline characters" ;;
esac
if printf '%s' "$keel_bin_dir" | LC_ALL=C grep -q '[[:cntrl:]]'; then
    fail "installation directory must not contain control characters"
fi
keel_bin_dir=${keel_bin_dir%/}
keel_target=$keel_bin_dir/keel
[ ! -L "$keel_target" ] || fail "refusing existing symlink target: $keel_target"
if [ -e "$keel_target" ] && [ ! -f "$keel_target" ]; then
    fail "existing target is not a regular file: $keel_target"
fi

case "$(uname -s)" in
    Linux) keel_os=Linux ;;
    Darwin) keel_os=macOS ;;
    *) fail "unsupported operating system; supported: Linux and macOS" ;;
esac
case "$(uname -m)" in
    x86_64|amd64) keel_arch=X64 ;;
    arm64|aarch64) keel_arch=ARM64 ;;
    *) fail "unsupported architecture; supported: x86_64 and arm64" ;;
esac
for keel_command in curl tar mktemp; do
    command -v "$keel_command" >/dev/null 2>&1 || fail "required command is unavailable: $keel_command"
done
if command -v sha256sum >/dev/null 2>&1; then
    keel_hash_command=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    keel_hash_command=shasum
else
    fail "sha256sum or shasum is required to verify the download"
fi

keel_tmp=
keel_stage=
cleanup() {
    keel_exit=$?
    trap - 0
    [ -z "$keel_stage" ] || rm -f "$keel_stage"
    [ -z "$keel_tmp" ] || rm -rf "$keel_tmp"
    exit "$keel_exit"
}
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
keel_tmp=$(mktemp -d "${TMPDIR:-/tmp}/keel-install.XXXXXXXX") || fail "cannot create temporary directory"
keel_asset=keel-$keel_os-$keel_arch.tar.gz
if [ "$keel_version" = latest ]; then
    keel_url=https://github.com/$keel_repo/releases/latest/download
else
    keel_url=https://github.com/$keel_repo/releases/download/$keel_version
fi
download() {
    curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
        --tlsv1.2 --connect-timeout 15 --max-time 120 --retry 2 --output "$2" "$1" ||
        fail "download failed: $1 (a matching published GitHub Release asset is required)"
}
printf 'Downloading %s from %s\n' "$keel_asset" "$keel_repo"
download "$keel_url/$keel_asset" "$keel_tmp/archive.tar.gz"
download "$keel_url/$keel_asset.sha256" "$keel_tmp/checksum"

# Accept the exact single-line format emitted by the CI package step, not an
# arbitrary checksum file that could name other paths.
keel_expected=$(awk 'NR == 1 { print $1 }' "$keel_tmp/checksum")
keel_checksum_name=$(awk 'NR == 1 { print $2 }' "$keel_tmp/checksum")
keel_checksum_lines=$(awk 'END { print NR }' "$keel_tmp/checksum")
keel_checksum_fields=$(awk 'NR == 1 { print NF }' "$keel_tmp/checksum")
[ "$keel_checksum_lines" = 1 ] && [ "$keel_checksum_fields" = 2 ] && [ "$keel_checksum_name" = "$keel_asset" ] ||
    fail "checksum file does not identify exactly the requested asset"
printf '%s\n' "$keel_expected" | LC_ALL=C grep -Eq '^[0-9a-fA-F]{64}$' ||
    fail "invalid SHA-256 checksum"
if [ "$keel_hash_command" = sha256sum ]; then
    keel_actual=$(sha256sum "$keel_tmp/archive.tar.gz" | awk '{ print $1 }')
else
    keel_actual=$(shasum -a 256 "$keel_tmp/archive.tar.gz" | awk '{ print $1 }')
fi
keel_expected=$(printf '%s' "$keel_expected" | tr 'A-F' 'a-f')
[ "$keel_actual" = "$keel_expected" ] || fail "SHA-256 checksum mismatch; existing installation is unchanged"

keel_entries=$(tar -tzf "$keel_tmp/archive.tar.gz") || fail "invalid archive"
[ "$keel_entries" = keel ] || fail "archive must contain exactly one file named keel"
# Stream the member to a file we created: never extract paths or symlinks from
# an archive into the destination filesystem.
tar -xOzf "$keel_tmp/archive.tar.gz" keel > "$keel_tmp/payload" || fail "cannot read compiler from archive"
[ -s "$keel_tmp/payload" ] || fail "archive contains an empty compiler"

mkdir -p "$keel_bin_dir" || fail "cannot create installation directory: $keel_bin_dir"
[ -d "$keel_bin_dir" ] && [ -w "$keel_bin_dir" ] || fail "installation directory is not writable"
[ ! -L "$keel_target" ] || fail "refusing existing symlink target: $keel_target"
if [ -e "$keel_target" ] && [ ! -f "$keel_target" ]; then
    fail "existing target is not a regular file: $keel_target"
fi
keel_stage=$(mktemp "$keel_bin_dir/.keel-install.XXXXXXXX") || fail "cannot stage installation"
cp "$keel_tmp/payload" "$keel_stage" || fail "cannot stage compiler"
chmod 755 "$keel_stage" || fail "cannot set compiler permissions"
mv -f "$keel_stage" "$keel_target" || fail "cannot install compiler"
keel_stage=
printf 'Installed Keel to %s\n' "$keel_target"
printf 'Add this directory to PATH if needed: %s\n' "$keel_bin_dir"
keel_quoted_bin=$(printf '%s' "$keel_bin_dir" | sed "s/'/'\\\\''/g")
printf "For this terminal: export PATH='%s':\"\$PATH\"\n" "$keel_quoted_bin"
printf 'Shell profiles were not changed. Try: keel --version\n'
printf 'Native builds also require a C compiler (cc, clang, or gcc).\n'
printf 'Next: keel doctor, then keel init hello-keel, cd hello-keel, keel run --allow-stdout\n'
