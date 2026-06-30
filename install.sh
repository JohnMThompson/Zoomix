#!/usr/bin/env bash
set -euo pipefail

readonly REPOSITORY="JohnMThompson/Zoomix"
readonly PACKAGE="zoomix_amd64.deb"
readonly RELEASE_BASE="https://github.com/${REPOSITORY}/releases/latest/download"

if ! command -v curl >/dev/null 2>&1; then
    printf 'Zoomix installer requires curl. Install it with: sudo apt install curl\n' >&2
    exit 1
fi

if ! command -v apt >/dev/null 2>&1 || ! command -v dpkg >/dev/null 2>&1; then
    printf 'Zoomix requires a Debian-based Linux distribution such as Linux Mint.\n' >&2
    exit 1
fi

architecture="$(dpkg --print-architecture)"
if [[ "${architecture}" != "amd64" ]]; then
    printf 'Zoomix currently provides packages for amd64 systems only (detected %s).\n' \
        "${architecture}" >&2
    exit 1
fi

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

printf 'Downloading the latest Zoomix release...\n'
curl --proto '=https' --tlsv1.2 --fail --location --show-error \
    --output "${temp_dir}/${PACKAGE}" "${RELEASE_BASE}/${PACKAGE}"
curl --proto '=https' --tlsv1.2 --fail --location --show-error \
    --output "${temp_dir}/${PACKAGE}.sha256" "${RELEASE_BASE}/${PACKAGE}.sha256"

printf 'Verifying package integrity...\n'
(
    cd "${temp_dir}"
    sha256sum --check "${PACKAGE}.sha256"
)

printf 'Installing Zoomix (administrator password may be required)...\n'
if [[ "${EUID}" -eq 0 ]]; then
    apt install --yes "${temp_dir}/${PACKAGE}"
elif command -v sudo >/dev/null 2>&1; then
    sudo apt install --yes "${temp_dir}/${PACKAGE}"
else
    printf 'Administrator access is required. Install sudo and try again.\n' >&2
    exit 1
fi

printf '\nZoomix is installed. Open it from the application menu.\n'
