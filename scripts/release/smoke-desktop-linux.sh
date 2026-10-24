#!/usr/bin/env bash
# Smoke test of the Linux x64 desktop packages users download (PRD 0021 RF6.2).
#   * AppImage and .deb: inside a clean, pinned Ubuntu 22.04 container (the
#     oldest supported glibc) — see smoke-desktop-linux-container.sh.
#   * .rpm: metadata (name, version, architecture, payload digests, the
#     installed binary) read by `rpm` in a pinned Fedora container with
#     `--network none`. Installing it pulls the whole WebKitGTK stack from
#     Fedora mirrors, so the rpm install stays in the manual checklist.
# Usage: smoke-desktop-linux.sh <dist-dir> <expected-version>
# Environment: DEB_IMAGE and RPM_IMAGE, container images pinned by digest.
set -euo pipefail

dist="${1:?dist directory}"
version="${2:?expected version}"
: "${DEB_IMAGE:?pinned Ubuntu image}" "${RPM_IMAGE:?pinned Fedora image}"
dist="$(cd "$dist" && pwd)"
scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

for name in "Cuotai_${version}_amd64.AppImage" "Cuotai_${version}_amd64.deb" "Cuotai_${version}_amd64.rpm"; do
  [ -f "$dist/$name" ] || { echo "missing $dist/$name" >&2; exit 1; }
done

rpm_file="Cuotai_${version}_amd64.rpm"
docker pull --quiet "$RPM_IMAGE" >/dev/null
read -r rpm_name rpm_version rpm_arch < <(
  docker run --rm --network none -v "$dist:/dist:ro" "$RPM_IMAGE" \
    rpm -qp --nosignature --qf '%{NAME} %{VERSION} %{ARCH}\n' "/dist/$rpm_file"
)
[ "$rpm_version" = "$version" ] || { echo "rpm Version is $rpm_version, expected $version" >&2; exit 1; }
[ "$rpm_arch" = "x86_64" ] || { echo "rpm Arch is $rpm_arch, expected x86_64" >&2; exit 1; }
docker run --rm --network none -v "$dist:/dist:ro" "$RPM_IMAGE" \
  rpm -K --nosignature "/dist/$rpm_file"
binaries="$(docker run --rm --network none -v "$dist:/dist:ro" "$RPM_IMAGE" \
  rpm -qlp --nosignature "/dist/$rpm_file" | grep -c '^/usr/bin/' || true)"
[ "$binaries" -ge 1 ] || { echo "the rpm installs nothing under /usr/bin" >&2; exit 1; }
echo "ok: rpm $rpm_name $rpm_version $rpm_arch (payload digests verified)"

docker pull --quiet "$DEB_IMAGE" >/dev/null
docker run --rm \
  -v "$dist:/dist:ro" -v "$scripts:/scripts:ro" \
  -e VERSION="$version" -e SMOKE_GRACE_SECONDS="${SMOKE_GRACE_SECONDS:-15}" \
  "$DEB_IMAGE" bash /scripts/smoke-desktop-linux-container.sh
echo "Linux desktop smoke passed for the AppImage, deb and rpm of $version"
