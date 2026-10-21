#!/usr/bin/env bash
# Writes the signing fragment for one CLI archive (PRD 0021 RF3.4): the file
# itself is unsigned; trust comes from the signed SHA256SUMS, resolved when
# the assembly merges the fragments. Usage: cli-fragment.sh <archive-path> <out-dir>
set -euo pipefail

archive="${1:?archive path}"
out_dir="${2:?fragments directory}"
name="$(basename "$archive")"
mkdir -p "$out_dir"

cat > "${out_dir}/cli-${name}.json" <<EOF
{
  "artifacts": [
    {
      "name": "${name}",
      "signed": false,
      "method": "checksum-signature",
      "status": "unsigned",
      "note": "unsigned payload; integrity is authenticated through SHA256SUMS when its signature is available"
    }
  ]
}
EOF
echo "fragment written for ${name}"
