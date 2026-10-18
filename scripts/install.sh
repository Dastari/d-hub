#!/bin/sh
# Install only an official stable d-hub GitHub release. No sudo or account access.
set -eu
umask 077
fail() { printf '%s\n' "d-hub: $*" >&2; exit 1; }
replace=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        --replace) replace=true ;;
        -h|--help)
            printf 'Usage: install.sh [--replace]\n\n--replace  Back up an existing user-owned launcher before installing.\n'
            exit 0 ;;
        *) fail "Unknown option: $1. Usage: install.sh [--replace]" ;;
    esac
    shift
done
[ "$(uname -s)" = Linux ] || fail 'Production builds currently support Linux only.'
case "$(uname -m)" in
    x86_64|amd64) target=x86_64-unknown-linux-gnu ;;
    aarch64|arm64) target=aarch64-unknown-linux-gnu ;;
    *) fail 'Unsupported CPU architecture.' ;;
esac
for utility in curl sha256sum mktemp install; do
    command -v "$utility" >/dev/null 2>&1 || fail "Install $utility first."
done
: "${HOME:?HOME must be set}"
root="${XDG_DATA_HOME:-$HOME/.local/share}/d-hub/bin"
bin="$HOME/.local/bin"
mkdir -p "$root" "$bin"
[ ! -L "$root" ] || fail 'Managed binary directory must not be a symbolic link.'
chmod 700 "$root"
# Serialize installers/updaters through the same flock file as the application.
command -v flock >/dev/null 2>&1 || fail 'Install util-linux (flock) first.'
mkdir -p "$root/.update-lock"
chmod 700 "$root/.update-lock"
exec 9>"$root/.update-lock/.demo.lock"
flock -n 9 || fail 'Another update is already running; retry shortly.'
check_launcher() {
    replace_launcher=false
    if [ -e "$bin/d-hub" ] || [ -L "$bin/d-hub" ]; then
        if [ -L "$bin/d-hub" ] && [ "$(readlink "$bin/d-hub")" = "$root/d-hub" ]; then
            return
        fi
        [ -f "$bin/d-hub" ] || [ -L "$bin/d-hub" ] || fail "Existing $bin/d-hub is not a regular file or symlink."
        [ "$replace" = true ] || fail "Existing $bin/d-hub found. To preserve it in a backup and install, rerun with: curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh -s -- --replace"
        [ "$(stat -c %u "$bin/d-hub")" = "$(id -u)" ] || fail 'Existing launcher must belong to the current user.'
        replace_launcher=true
    fi
}
check_launcher
work=$(mktemp -d "$root/.install.XXXXXXXX")
link_work=
backup=
trap 'rm -rf "$work"; if [ -n "$link_work" ]; then rm -rf "$link_work"; fi' EXIT HUP INT TERM
base=https://github.com/dastari/d-hub/releases
# Resolve latest once, then download every file from the same immutable version path.
release_url=$(curl --proto '=https' --tlsv1.2 -fsSL --proto-redir '=https' --max-time 30 -o /dev/null -w '%{url_effective}' "$base/latest")
tag=${release_url##*/}
case "$release_url" in
    https://github.com/dastari/d-hub/releases/tag/v*|https://github.com/Dastari/d-hub/releases/tag/v*) ;;
    *) fail 'Unexpected release redirect.' ;;
esac
printf '%s\n' "$tag" | LC_ALL=C grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Latest release is not a stable version.'
asset="d-hub-$target"
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --max-time 180 "$base/download/$tag/$asset" -o "$work/$asset"
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --max-time 30 "$base/download/$tag/SHA256SUMS" -o "$work/SHA256SUMS"
expected=$(awk -v name="$asset" '$2 == name && length($1) == 64 { print $1 }' "$work/SHA256SUMS")
[ -n "$expected" ] || fail 'Release checksum missing.'
(cd "$work" && printf '%s  %s\n' "$expected" "$asset" | sha256sum -c -) || fail 'Checksum verification failed.'
chmod 755 "$work/$asset"
[ "$("$work/$asset" --version)" = "d-hub ${tag#v}" ] || fail 'Binary cannot run or has the wrong version.'
# Recheck after the download. Preserve the existing entry only after validation;
# copying a symlink keeps its target untouched, including dangling links.
check_launcher
if [ "$replace_launcher" = true ]; then
    backup=$(mktemp -d "$bin/d-hub-backup.XXXXXXXX")
    cp -Pp "$bin/d-hub" "$backup/d-hub"
fi
# Stage the launcher on its own filesystem, then atomically replace the entry.
link_work=$(mktemp -d "$bin/.d-hub-link.XXXXXXXX")
ln -s "$root/d-hub" "$link_work/d-hub"
if [ -f "$root/d-hub" ]; then
    cp -p "$root/d-hub" "$work/previous"
    mv -f "$work/previous" "$root/d-hub.previous"
fi
mv -f "$work/$asset" "$root/d-hub"
printf 'd-hub-managed-v1\n' > "$work/marker"
mv -f "$work/marker" "$root/.d-hub-managed"
mv -Tf "$link_work/d-hub" "$bin/d-hub"
if [ -n "$backup" ]; then
    printf '\nPrevious launcher saved at: %s/d-hub\n' "$backup"
fi
printf '\nInstalled d-hub %s to %s\nRun: %s/d-hub\n' "${tag#v}" "$root" "$bin"
case ":$PATH:" in *":$bin:"*) ;; *) printf '\nAdd to your shell profile: export PATH="$HOME/.local/bin:$PATH"\n' ;; esac
printf '\nManaged installations update automatically. A running service hands over safely on its next check; use :update to check now.\n'
