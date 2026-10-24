#!/usr/bin/env bash
# Runs inside a clean Ubuntu 22.04 container started by smoke-desktop-linux.sh
# (PRD 0021 RF6.2). Nothing from the build host is present: only the packages
# under /dist and what apt resolves for them.
#   1. .deb: installed with apt (which resolves its declared dependencies);
#      the package metadata must carry the release version and amd64; `ldd`
#      must resolve every library; it must start under Xvfb.
#   2. AppImage: extracted without FUSE (`--appimage-extract`); `ldd` must
#      resolve every library of the main binary from the bundle or from the
#      desktop base installed in step 1; it must start under Xvfb.
# "Starts" means still running after a grace period (SMOKE_GRACE_SECONDS).
set -euo pipefail

: "${VERSION:?}"
grace="${SMOKE_GRACE_SECONDS:-15}"
appimage="/dist/Cuotai_${VERSION}_amd64.AppImage"
deb="/dist/Cuotai_${VERSION}_amd64.deb"
export DEBIAN_FRONTEND=noninteractive
# WebKitGTK's bubblewrap sandbox needs user namespaces an unprivileged
# container does not have; this disposable container is the only place it is
# turned off.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export HOME=/tmp/smoke-home
mkdir -p "$HOME"

apt_get() {
  apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30 "$@"
}

missing_libraries() { # <binary> [library path]
  LD_LIBRARY_PATH="${2:-}" ldd "$1" | grep 'not found' || true
}

assert_starts() { # <label> <command...>
  local label="$1"
  shift
  local code=0
  timeout "$grace" dbus-run-session -- xvfb-run -a "$@" >"/tmp/$label.log" 2>&1 || code=$?
  if [ "$code" -ne 124 ]; then
    echo "$label exited with $code within ${grace}s of starting" >&2
    tail -n 60 "/tmp/$label.log" >&2
    exit 1
  fi
  echo "ok: $label is running after ${grace}s"
}

apt_get update -qq
apt_get install -y -qq --no-install-recommends xvfb xauth dbus >/dev/null

# 1. .deb on the clean base.
# shellcheck disable=SC2016 # dpkg-deb expands its own ${Field} names.
read -r package deb_version deb_arch < <(dpkg-deb -W --showformat='${Package} ${Version} ${Architecture}\n' "$deb")
[ "$deb_version" = "$VERSION" ] || { echo "deb Version is $deb_version, expected $VERSION" >&2; exit 1; }
[ "$deb_arch" = "amd64" ] || { echo "deb Architecture is $deb_arch, expected amd64" >&2; exit 1; }
apt_get install -y -qq --no-install-recommends "$deb" >/dev/null
dpkg -s "$package" | grep -q '^Status: install ok installed' || { echo "$package is not installed" >&2; exit 1; }
mapfile -t deb_bins < <(dpkg -L "$package" | grep '^/usr/bin/' || true)
[ "${#deb_bins[@]}" -ge 1 ] || { echo "$package installs nothing under /usr/bin" >&2; exit 1; }
missing="$(missing_libraries "${deb_bins[0]}")"
if [ -n "$missing" ]; then
  echo "${deb_bins[0]} has unresolved libraries after installing $package:" >&2
  echo "$missing" >&2
  exit 1
fi
echo "ok: deb $package $deb_version $deb_arch installed; ldd resolves every library of ${deb_bins[0]}"
assert_starts deb "${deb_bins[0]}"

# 2. AppImage. linuxdeploy deliberately leaves the libraries every desktop
# has (fontconfig, HarfBuzz, FriBiDi, Wayland, EGL, GBM) to the host, so the
# check runs on the system the .deb dependencies just turned into a desktop
# base; everything else must come from the bundle.
mkdir -p /tmp/appimage
cp "$appimage" /tmp/appimage/Cuotai.AppImage
chmod +x /tmp/appimage/Cuotai.AppImage
(cd /tmp/appimage && ./Cuotai.AppImage --appimage-extract >/dev/null)
root=/tmp/appimage/squashfs-root
[ -x "$root/AppRun" ] || { echo "the AppImage has no AppRun" >&2; exit 1; }
shopt -s nullglob
bins=("$root"/usr/bin/*)
shopt -u nullglob
[ "${#bins[@]}" -ge 1 ] || { echo "the AppImage has no usr/bin binary" >&2; exit 1; }
libpath="$root/usr/lib:$root/usr/lib/x86_64-linux-gnu:$root/usr/lib64"
missing="$(missing_libraries "${bins[0]}" "$libpath")"
if [ -n "$missing" ]; then
  echo "AppImage binary $(basename "${bins[0]}") has unresolved libraries:" >&2
  echo "$missing" >&2
  exit 1
fi
echo "ok: AppImage extracted; ldd resolves every library of $(basename "${bins[0]}")"
assert_starts appimage "$root/AppRun"
