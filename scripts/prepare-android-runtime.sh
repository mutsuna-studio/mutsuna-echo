#!/usr/bin/env bash

set -euo pipefail

readonly SHERPA_ONNX_VERSION="1.13.4"
readonly ABI="${1:-arm64-v8a}"
readonly ARCHIVE_NAME="sherpa-onnx-v${SHERPA_ONNX_VERSION}-android.tar.bz2"
readonly RELEASE_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/v${SHERPA_ONNX_VERSION}/${ARCHIVE_NAME}"
readonly RELEASE_API="https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/v${SHERPA_ONNX_VERSION}"
readonly REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly CACHE_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mutsuna-echo-sherpa-onnx-v${SHERPA_ONNX_VERSION}"
readonly ARCHIVE_PATH="${CACHE_ROOT}/${ARCHIVE_NAME}"
readonly EXTRACTED_PATH="${CACHE_ROOT}/extracted"
readonly DESTINATION="${REPOSITORY_ROOT}/src-tauri/gen/android/local_ai_runtime/src/main/jniLibs/${ABI}"

case "${ABI}" in
  arm64-v8a|armeabi-v7a|x86|x86_64) ;;
  *)
    echo "Unsupported Android ABI: ${ABI}" >&2
    exit 1
    ;;
esac

mkdir -p "${CACHE_ROOT}"
if [[ ! -f "${ARCHIVE_PATH}" ]]; then
  curl --fail --location --retry 5 --retry-all-errors \
    --output "${ARCHIVE_PATH}.part" "${RELEASE_URL}"
  mv "${ARCHIVE_PATH}.part" "${ARCHIVE_PATH}"
fi

release_json="$(curl --fail --location --retry 5 --retry-all-errors \
  --header "Accept: application/vnd.github+json" \
  --header "X-GitHub-Api-Version: 2022-11-28" \
  "${RELEASE_API}")"
release_digest="$(printf '%s' "${release_json}" | node -e '
  const fs = require("node:fs");
  const archiveName = process.argv[1];
  const release = JSON.parse(fs.readFileSync(0, "utf8"));
  process.stdout.write(release.assets.find((asset) => asset.name === archiveName)?.digest ?? "");
' "${ARCHIVE_NAME}")"

if [[ ! "${release_digest}" =~ ^sha256:([0-9a-f]{64})$ ]]; then
  echo "GitHub did not return a SHA-256 digest for ${ARCHIVE_NAME}." >&2
  exit 1
fi
echo "${BASH_REMATCH[1]}  ${ARCHIVE_PATH}" | sha256sum --check --status

if [[ ! -d "${EXTRACTED_PATH}" ]]; then
  mkdir -p "${EXTRACTED_PATH}.part"
  tar -xjf "${ARCHIVE_PATH}" -C "${EXTRACTED_PATH}.part"
  mv "${EXTRACTED_PATH}.part" "${EXTRACTED_PATH}"
fi

find_runtime_library() {
  local name="$1"
  local matches=()
  mapfile -t matches < <(find "${EXTRACTED_PATH}" -type f \
    -path "*/${ABI}/${name}" -print)
  if [[ "${#matches[@]}" -ne 1 ]]; then
    echo "Expected exactly one ${ABI}/${name} in ${ARCHIVE_NAME}; found ${#matches[@]}." >&2
    exit 1
  fi
  printf '%s\n' "${matches[0]}"
}

sherpa_library="$(find_runtime_library libsherpa-onnx-c-api.so)"
onnxruntime_library="$(find_runtime_library libonnxruntime.so)"
mkdir -p "${DESTINATION}"
cp "${sherpa_library}" "${onnxruntime_library}" "${DESTINATION}/"

printf 'Sherpa ONNX Android runtime: %s\n' "${DESTINATION}"
