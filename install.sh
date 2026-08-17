#!/bin/sh
# celsius installer.
#
#   curl -fsSL https://raw.githubusercontent.com/lmarkmann/celsius/main/install.sh | sh
#
# Options, each also settable as an environment variable. When piping, options go after `-s --`, because a bare `| sh --install-dir X` passes the option to sh itself:
#
#   curl -fsSL https://raw.githubusercontent.com/lmarkmann/celsius/main/install.sh | sh -s -- --install-dir ~/bin
#
#   --version <tag>      CELSIUS_VERSION       release tag to install, e.g. v0.5.0 (default: latest)
#   --install-dir <dir>  CELSIUS_INSTALL_DIR   where the binary goes (default: $XDG_BIN_HOME, else ~/.local/bin)
#                        CELSIUS_BASE_URL      https download origin override, for testing
#                        CELSIUS_ALLOW_SUDO    permit running under sudo
#                        NO_COLOR              suppress colour, per the NO_COLOR spec
#
# Downloads the prebuilt binary for this platform from GitHub Releases, verifies the published SHA-256, runs it once from a temporary directory, and only then installs it. No sudo, no prompts, nothing written outside the install directory, no shell rc files touched. Prebuilt binaries exist for arm64 macOS and x86_64 Linux; every other platform gets told so and pointed at `cargo install celsius`.

# Every command that matters is checked explicitly through `ensure`, which reports the command that failed. `set -e` is deliberately not used: its behaviour inside `if`, `&&` and command substitution is too surprising to rely on for install logic.
set -u

REPO="lmarkmann/celsius"
BIN="celsius"

# The whole script is functions, and main is called on the last line. If the connection drops mid-transfer, `sh` executes a truncated file that defines some functions and then reaches EOF without calling anything, instead of running half an installation.

setup_output() {
    if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
        _c_bold=$(printf '\033[1m')
        _c_red=$(printf '\033[31m')
        _c_yellow=$(printf '\033[33m')
        _c_off=$(printf '\033[0m')
    else
        _c_bold='' _c_red='' _c_yellow='' _c_off=''
    fi
}

say() { printf '%s\n' "$*" >&2; }
warn() { printf '%swarning:%s %s\n' "$_c_yellow" "$_c_off" "$*" >&2; }

err() {
    printf '%serror:%s %s\n' "$_c_red" "$_c_off" "$*" >&2
    exit 1
}

check_cmd() { command -v "$1" >/dev/null 2>&1; }
need_cmd() { check_cmd "$1" || err "this installer needs \`$1\` and it is not on your PATH"; }
ensure() { "$@" || err "command failed: $*"; }

# A function rather than a trap string, because the string form has to interpolate the paths and an apostrophe in $HOME then breaks the quoting. The INT and TERM handlers exit; a handler that only cleans up lets POSIX sh resume, which turns the user's own Ctrl-C into a download-failed message.
cleanup() {
    [ -n "${_tmp:-}" ] && rm -rf "$_tmp"
    [ -n "${_staged:-}" ] && rm -f "$_staged"
    return 0
}

# curl and wget are both accepted because minimal container images ship one or the other. curl is preferred wherever it exists: `--proto '=https'` applies to redirects as well as the initial request, so it stops a redirect from downgrading the transfer to a scheme the shell would execute from. Neither wget can promise that, so the wget path is a fallback rather than an equal, and it exists because Alpine ships busybox wget and no curl at all.
detect_downloader() {
    _dl=''
    if check_cmd curl; then
        _dl=curl
        # A snap-confined curl cannot write outside $HOME, which breaks --output into a temp dir.
        case "$(command -v curl)" in
        /snap/*)
            if check_cmd wget; then
                _dl=wget
            else
                warn "curl is snap-confined and may not be able to write outside \$HOME"
            fi
            ;;
        esac
    elif check_cmd wget; then
        _dl=wget
    fi
    [ -n "$_dl" ] || err "this installer needs curl or wget and found neither"
}

# The timeouts exist because the common CI failure is not a refused connection, it is a hung one. --max-time alone does not bound the run: curl resets that counter on every retry, so it needs --retry-max-time to cap the whole attempt, and --speed-limit to abandon a transfer that is technically alive and moving nowhere.
# The wget flags are the short forms busybox accepts. GNU-only long options (--https-only, --max-redirect, --tries) abort busybox wget with an unrecognized-option error, which the caller would then report as a network or release problem.
download() {
    case "$_dl" in
    curl)
        curl --proto '=https' --location --fail --silent --show-error \
            --connect-timeout 10 --max-time 300 --max-redirs 5 \
            --retry 3 --retry-max-time 120 --speed-limit 1024 --speed-time 20 \
            --output "$2" -- "$1"
        ;;
    wget)
        wget -q -T 30 -O "$2" -- "$1"
        ;;
    esac
}

# Resolving the tag from the /releases/latest redirect rather than api.github.com, which is rate limited to 60 requests per hour per IP and so fails for anyone behind a shared NAT at exactly the wrong moment. Both branches follow that redirect, so both skip prereleases; the releases atom feed is smaller but lists them.
latest_tag() {
    case "$_dl" in
    curl)
        _loc=$(curl --proto '=https' --location --fail --silent --show-error \
            --connect-timeout 10 --max-time 60 --max-redirs 5 \
            --retry 2 --retry-max-time 60 \
            --head --output /dev/null --write-out '%{url_effective}' \
            -- "https://github.com/$REPO/releases/latest") || return 1
        printf '%s\n' "${_loc##*/}"
        ;;
    wget)
        # No wget reports its redirect target portably, so this reads the tag out of the page the redirect lands on. The pattern tolerates the href being relative or absolute.
        download "https://github.com/$REPO/releases/latest" "$1" || return 1
        sed -n 's|.*/releases/tag/\(v[0-9][^"]*\)".*|\1|p' "$1" | head -n 1
        ;;
    esac
}

detect_target() {
    _target=''
    _os=$(uname -s)
    _arch=$(uname -m)

    case "$_os" in
    Linux)
        # A 32-bit userland on a 64-bit kernel still reports x86_64 from uname.
        if [ "$(getconf LONG_BIT 2>/dev/null || echo 64)" = 32 ]; then
            err "this is a 32-bit userland, which has no prebuilt binary. Build it: cargo install celsius"
        fi
        # Linux is built as static musl, so there is no glibc version floor to check and no separate gnu artifact to choose between. ARM Linux ships no binary and falls through to the message at the end.
        case "$_arch" in
        x86_64 | amd64) _target=x86_64-unknown-linux-musl ;;
        esac
        ;;
    Darwin)
        # An x86_64 shell under Rosetta reports x86_64, and would otherwise be turned away as an Intel Mac. sysctl lives in /usr/sbin, which pruned PATHs drop.
        _sysctl=$(command -v sysctl 2>/dev/null || echo /usr/sbin/sysctl)
        if [ "$_arch" = x86_64 ] &&
            [ "$("$_sysctl" -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
            _arch=arm64
        fi
        # Intel Mac ships no binary; the Homebrew formula covers it with a source build.
        case "$_arch" in
        arm64) _target=aarch64-apple-darwin ;;
        esac
        ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT)
        err "this installer does not handle Windows, but the release does publish a Windows binary.
  Download celsius-<tag>-x86_64-pc-windows-msvc.zip from https://github.com/$REPO/releases/latest
  or build it: cargo install celsius"
        ;;
    esac

    [ -n "$_target" ] ||
        err "no prebuilt binary for $_os/$_arch. Build it: cargo install celsius
  If you think this platform should be supported, open an issue at https://github.com/$REPO/issues"
}

verify_checksum() {
    _archive_path="$1"
    _sum_path="$2"

    _expected=$(awk 'NR == 1 { print $1 }' "$_sum_path")
    # A proxy error page or a truncated transfer frequently starts with a hex character, so a first-character test lets one through and it then fails as a mismatch, which reads as tampering rather than as a broken download. Uppercase is rejected too, since every digest below is lowercase and an uppercase one could only ever mismatch.
    _malformed=''
    case "$_expected" in
    *[!0-9a-f]*) _malformed=yes ;;
    esac
    if [ -n "$_malformed" ] || [ "${#_expected}" -ne 64 ]; then
        err "what was published as the checksum for this release is not a SHA-256 digest, so the download cannot be verified.
  Usually this means a proxy answered with an error page instead of the file."
    fi

    if check_cmd sha256sum; then
        _actual=$(sha256sum "$_archive_path" | awk '{ print $1 }')
    elif check_cmd shasum; then
        _actual=$(shasum -a 256 "$_archive_path" | awk '{ print $1 }')
    elif check_cmd openssl; then
        _actual=$(openssl dgst -sha256 "$_archive_path" | awk '{ print $NF }')
    else
        err "no sha256sum, shasum or openssl available, so the download cannot be verified"
    fi

    if [ "$_actual" != "$_expected" ]; then
        err "checksum mismatch, refusing to install
  expected $_expected
  got      $_actual"
    fi
}

report_path() {
    _dir="$1"
    case ":$PATH:" in
    *":$_dir:"*) ;;
    *)
        say ""
        say "$_dir is not on your PATH. Add it:"
        say "  fish:      fish_add_path $_dir"
        say "  bash/zsh:  export PATH=\"$_dir:\$PATH\""
        return 0
        ;;
    esac

    # Present on PATH, but an older copy from brew or cargo may still win.
    _shadow=$(command -v "$BIN" 2>/dev/null || true)
    if [ -n "$_shadow" ] && [ "$_shadow" != "$_dir/$BIN" ]; then
        warn "$_shadow comes earlier on your PATH, so \`$BIN\` still runs that copy"
    fi
}

usage() {
    cat <<EOF
Install celsius, a terminal weather TUI.

  --version <tag>       install a specific release, e.g. v0.5.0
  --install-dir <dir>   install location (default: \$XDG_BIN_HOME, else \$HOME/.local/bin)
  -h, --help            show this message

Piping needs -s -- before the options:
  curl -fsSL <url>/install.sh | sh -s -- --install-dir ~/bin
EOF
}

main() {
    setup_output

    _version="${CELSIUS_VERSION:-}"
    _install_dir="${CELSIUS_INSTALL_DIR:-}"

    while [ $# -gt 0 ]; do
        case "$1" in
        --version) [ $# -ge 2 ] || err "--version needs a tag"; _version="$2"; shift 2 ;;
        --install-dir) [ $# -ge 2 ] || err "--install-dir needs a path"; _install_dir="$2"; shift 2 ;;
        -h | --help) usage; exit 0 ;;
        *) err "unknown option: $1" ;;
        esac
    done

    # A $HOME install under sudo lands in /root and leaves the real user with nothing.
    if [ "$(id -u)" = 0 ] && [ -n "${SUDO_USER:-}" ] && [ "${SUDO_USER:-}" != root ] &&
        [ -z "${CELSIUS_ALLOW_SUDO:-}" ]; then
        err "do not run this under sudo; it installs into your home directory
  Run it as yourself, or set CELSIUS_ALLOW_SUDO=1 if you meant it"
    fi

    # $HOME is only needed to build the default, so a container with no HOME can still install by naming a directory.
    if [ -z "$_install_dir" ]; then
        [ -n "${HOME:-}" ] || err "\$HOME is not set; pass --install-dir explicitly"
        _install_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
    fi
    # A quoted CELSIUS_INSTALL_DIR='~/bin' never reaches tilde expansion, and would otherwise create a literal ~ directory wherever the script was piped from.
    # shellcheck disable=SC2088 # the tilde is meant to stay literal here; this branch exists to detect the one that was never expanded
    case "$_install_dir" in
    "~" | "~/"*)
        [ -n "${HOME:-}" ] || err "cannot expand ~ because \$HOME is not set; pass an absolute --install-dir"
        _install_dir="$HOME${_install_dir#\~}"
        ;;
    esac

    need_cmd uname
    need_cmd tar
    need_cmd mktemp
    need_cmd awk
    need_cmd sed
    # Checked here rather than where the digest is computed, so a machine with none of the three is turned away before the download instead of after it. verify_checksum keeps its own dispatch and its own error, which is now unreachable but stays total.
    check_cmd sha256sum || check_cmd shasum || check_cmd openssl ||
        err "this installer needs sha256sum, shasum or openssl to verify the download, and found none"
    detect_downloader
    detect_target

    # Everything the install directory can be wrong about is settled here, before any of it is paid for over the network. mkdir -p says nothing about a directory that already exists and is root-owned, which is what /usr/local/bin is on most machines.
    ensure mkdir -p "$_install_dir"
    _install_dir=$(cd "$_install_dir" && pwd) ||
        err "could not enter the install directory"
    [ -w "$_install_dir" ] ||
        err "$_install_dir is not writable by you
  Install somewhere you own, for example --install-dir \"\$HOME/.local/bin\""
    if [ -d "$_install_dir/$BIN" ]; then
        err "$_install_dir/$BIN is a directory, so nothing can be installed under that name
  Move it aside, or choose another --install-dir"
    fi

    _tmp=$(mktemp -d) || err "could not create a temporary directory"
    trap cleanup EXIT
    trap 'cleanup; exit 130' INT
    trap 'cleanup; exit 143' TERM
    ensure mkdir -p "$_tmp/unpack"

    if [ -z "$_version" ]; then
        _version=$(latest_tag "$_tmp/latest.html")
        [ -n "$_version" ] || err "could not resolve the latest release from GitHub
  Retry, or name one: --version v0.5.0"
    fi
    # Applied to every source of the tag, not only the resolved one: crates.io, the changelog and `cargo install --version` all spell it without the v, and that spelling would otherwise 404 and be reported as an unsupported platform.
    #
    # Two checks rather than one, because the shape check alone is far looser than it reads: `*` in a glob matches metacharacters, so `v[0-9]*.[0-9]*.[0-9]*` accepts `v0.5.0; rm -rf /` and `v0.5.0/../..` as readily as `v0.5.0`. Nothing here is interpolated into a command and every use is quoted, so that is not an execution path, but the tag does become a URL and a filename component, and on the wget branch it is parsed out of a downloaded page. Rejecting everything but digits and dots keeps it safe to use as both.
    case "$_version" in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) err "\`$_version\` is not a release tag; they look like v0.5.0
  Releases: https://github.com/$REPO/releases" ;;
    esac
    case "${_version#v}" in
    *[!0-9.]*) err "\`$_version\` is not a release tag; they look like v0.5.0
  Releases: https://github.com/$REPO/releases" ;;
    esac

    _base="${CELSIUS_BASE_URL:-https://github.com/$REPO/releases/download}"
    _stem="$BIN-$_version-$_target"
    _archive="$_stem.tar.gz"

    # The checksum is named off the archive stem, not the archive: release.yml hands upload-rust-binary-action `archive: $bin-$tag-$target` and the action appends .sha256 to that template, so the published asset is celsius-v0.5.0-<target>.sha256 and not celsius-v0.5.0-<target>.tar.gz.sha256. Fetching it first also means a tag with no artifacts for this platform fails after a hundred bytes instead of after a 2 MB download.
    say "downloading $_archive"
    download "$_base/$_version/$_stem.sha256" "$_tmp/$_stem.sha256" ||
        err "no checksum published for $_archive, refusing to install
  $_version may predate the current release layout, or may not build for $_target"
    download "$_base/$_version/$_archive" "$_tmp/$_archive" ||
        err "download failed: $_base/$_version/$_archive"
    verify_checksum "$_tmp/$_archive" "$_tmp/$_stem.sha256"

    # --no-same-owner keeps a root-run extraction from restoring archived ownership, and unpacking into its own directory means a tarbomb cannot reach anything else.
    ensure tar -xzf "$_tmp/$_archive" --no-same-owner -C "$_tmp/unpack"
    [ -f "$_tmp/unpack/$BIN" ] || err "the archive did not contain a $BIN binary"
    ensure chmod 0755 "$_tmp/unpack/$BIN"

    # Run it before installing it. A wrong-architecture or truncated binary fails here, while whatever is already installed is still untouched.
    _reported=$("$_tmp/unpack/$BIN" --version 2>/dev/null) ||
        err "the downloaded binary would not run, so nothing was installed
  If your temporary directory is mounted noexec, retry with TMPDIR=\"\$HOME/.cache\" set"

    # Stage on the destination filesystem and rename: an atomic replace succeeds even while the old binary is running, where writing in place gives ETXTBSY on Linux.
    _staged="$_install_dir/.$BIN.new.$$"
    ensure cp "$_tmp/unpack/$BIN" "$_staged"
    ensure chmod 0755 "$_staged"
    ensure mv -f "$_staged" "$_install_dir/$BIN"
    _staged=''

    say "${_c_bold}installed $_reported to $_install_dir/$BIN${_c_off}"
    report_path "$_install_dir"
}

main "$@"
