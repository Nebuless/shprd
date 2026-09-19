#!/bin/sh
set -eu

github_repository="Nebuless/shprd"
custom_release_base="${SHPRD_RELEASE_BASE_URL:-${HERDR_GUI_RELEASE_BASE_URL:-}}"
install_dir="${SHPRD_INSTALL_DIR:-${HERDR_GUI_INSTALL_DIR:-$HOME/.local/bin}}"
requested_version="${SHPRD_VERSION:-${HERDR_GUI_VERSION:-}}"

fail() {
  printf 'shprd installer: %s\n' "$*" >&2
  exit 1
}

for command in curl tar install uname awk cat mktemp; do
  command -v "$command" >/dev/null 2>&1 ||
    fail "required command not found: $command"
done
if command -v shasum >/dev/null 2>&1; then
  checksum_tool="shasum"
elif command -v sha256sum >/dev/null 2>&1; then
  checksum_tool="sha256sum"
else
  fail "required command not found: shasum or sha256sum"
fi

case "$(uname -s):$(uname -m)" in
  Darwin:arm64 | Darwin:aarch64)
    platform="darwin-arm64"
    ;;
  Darwin:x86_64 | Darwin:amd64)
    platform="darwin-x64"
    ;;
  Linux:x86_64 | Linux:amd64)
    platform="linux-x64"
    ;;
  Linux:arm64 | Linux:aarch64)
    platform="linux-arm64"
    ;;
  *)
    fail "unsupported platform: $(uname -s) $(uname -m)"
    ;;
esac

if [ -n "$requested_version" ]; then
  case "$requested_version" in
    *[!0-9A-Za-z._-]*)
      fail "invalid SHPRD_VERSION: $requested_version"
      ;;
  esac
  archive_name="shprd-v${requested_version}-${platform}.tar.xz"
else
  archive_name="shprd-${platform}.tar.xz"
fi

# GitHub uses a different asset directory for latest and versioned releases.
# Custom mirrors keep the existing flat-directory contract.
if [ -n "$custom_release_base" ]; then
  release_base="$custom_release_base"
elif [ -n "$requested_version" ]; then
  release_base="https://github.com/$github_repository/releases/download/v${requested_version}"
else
  release_base="https://github.com/$github_repository/releases/latest/download"
fi
while [ "${release_base%/}" != "$release_base" ]; do
  release_base="${release_base%/}"
done
case "$release_base" in
  *\?* | *\#*)
    fail "release base URL must not contain a query or fragment"
    ;;
esac
release_authority="${release_base#*://}"
release_authority="${release_authority%%/*}"
[ -n "$release_authority" ] || fail "invalid release base URL"
case "$release_authority" in
  *@*) fail "release base URL must not contain credentials" ;;
esac
case "$release_base" in
  https://*)
    curl_protocol="=https"
    ;;
  http://*)
    case "$release_authority" in
      localhost | localhost:* | 127.0.0.1 | 127.0.0.1:* | "[::1]" | "[::1]":*) ;;
      *) fail "release base URL must use HTTPS unless the mirror is loopback" ;;
    esac
    curl_protocol="=http"
    ;;
  *) fail "release base URL must be an HTTP(S) URL" ;;
esac
package_dir="shprd-${platform}"
mkdir -p "$install_dir"
target="$install_dir/shprd"
if { [ -e "$target" ] || [ -L "$target" ]; } &&
  { [ ! -f "$target" ] || [ -L "$target" ]; }; then
  fail "install target exists but is not a regular file"
fi
home_staging_dir="${HOME:?HOME must be set}/.local/share/shprd/tmp"
staging_dir="${SHPRD_TEMP_DIR:-$home_staging_dir}"
tmp=""
target_tmp=""
backup_tmp=""

cleanup() {
  if [ -n "${tmp:-}" ]; then
    command -p rm -rf "$tmp"
  fi
  if [ -n "${target_tmp:-}" ]; then
    command -p rm -f "$target_tmp"
  fi
  if [ -n "${backup_tmp:-}" ]; then
    command -p rm -f "$backup_tmp"
  fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

create_staging_dir() {
  candidate="$1"
  [ -d "$candidate" ] || return 1
  tmp="$(mktemp -d "$candidate/shprd-install.XXXXXX")" || {
    tmp=""
    return 1
  }
}

use_home_staging_dir() {
  if [ "$staging_dir" = "$home_staging_dir" ]; then
    fail "cannot create temporary download directory: $home_staging_dir"
  fi
  mkdir -p "$home_staging_dir" ||
    fail "cannot create temporary download directory: $home_staging_dir"
  tmp=""
  create_staging_dir "$home_staging_dir" ||
    fail "cannot create temporary download directory: $home_staging_dir"
  staging_dir="$home_staging_dir"
  printf 'Custom temporary directory unavailable; using %s for installation downloads.\n' \
    "$home_staging_dir" >&2
}

if [ "$staging_dir" = "$home_staging_dir" ]; then
  mkdir -p "$home_staging_dir" ||
    fail "cannot create temporary download directory: $home_staging_dir"
  create_staging_dir "$home_staging_dir" ||
    fail "cannot create temporary download directory: $home_staging_dir"
elif ! create_staging_dir "$staging_dir"; then
  use_home_staging_dir
fi

download_release_assets() {
  archive="$tmp/$archive_name"
  checksum="$archive.sha256"
  curl --proto "$curl_protocol" --proto-redir "$curl_protocol" \
    -fsSL "$release_base/$archive_name" -o "$archive" &&
    curl --proto "$curl_protocol" --proto-redir "$curl_protocol" \
      --max-filesize 4096 -fsSL \
      "$release_base/$archive_name.sha256" -o "$checksum"
}

printf 'Downloading SHPRD for %s...\n' "$platform"
if ! download_release_assets; then
  command -p rm -rf "$tmp"
  tmp=""
  use_home_staging_dir
  download_release_assets || fail "release download failed"
fi
checksum_line="$(cat "$checksum")" || fail "unable to read package checksum"
case "$checksum_line" in
  *"  "*)
    expected_checksum="${checksum_line%% *}"
    checksum_name="${checksum_line#*  }"
    ;;
  *) fail "invalid package checksum file" ;;
esac
[ -n "$expected_checksum" ] || fail "invalid package checksum file"
[ -n "$checksum_name" ] || fail "invalid package checksum file"
case "$checksum_name" in
  *" "*) fail "invalid package checksum file" ;;
esac
[ "${#expected_checksum}" -eq 64 ] || fail "invalid package checksum file"
case "$expected_checksum" in
  *[!0-9A-Fa-f]*) fail "invalid package checksum file" ;;
esac
expected_checksum="$(printf '%s\n' "$expected_checksum" | awk '{ print tolower($0) }')"
[ "$checksum_name" = "$archive_name" ] || fail "invalid package checksum file"
if [ "$checksum_tool" = "shasum" ]; then
  actual_checksum="$(shasum -a 256 "$archive" | awk 'NR == 1 { print $1 }')"
else
  actual_checksum="$(sha256sum "$archive" | awk 'NR == 1 { print $1 }')"
fi
[ "$actual_checksum" = "$expected_checksum" ] || fail "package checksum mismatch"

tar -xJf "$archive" -C "$tmp" \
  "$package_dir/VERSION" \
  "$package_dir/shprd"

extracted_package_dir="$tmp/$package_dir"
version_file="$extracted_package_dir/VERSION"
binary="$extracted_package_dir/shprd"
[ -d "$extracted_package_dir" ] && [ ! -L "$extracted_package_dir" ] ||
  fail "package directory is invalid"
[ -f "$version_file" ] && [ ! -L "$version_file" ] ||
  fail "package VERSION file is missing or invalid"
[ -f "$binary" ] && [ ! -L "$binary" ] && [ -x "$binary" ] ||
  fail "package binary is missing, invalid, or not executable"

package_name=""
package_version=""
package_platform=""
extra_version_field=""
read -r package_name package_version package_platform extra_version_field \
  <"$version_file" || fail "invalid package VERSION file"
[ "$package_name" = "shprd" ] || fail "invalid package VERSION file"
[ -z "$extra_version_field" ] || fail "invalid package VERSION file"
[ -n "$package_version" ] || fail "package version is missing"
[ "$package_platform" = "$platform" ] ||
  fail "package platform is $package_platform, expected $platform"
[ -z "$requested_version" ] || [ "$package_version" = "$requested_version" ] ||
  fail "package version is $package_version, expected $requested_version"

binary_version="$("$binary" --version)"
[ "$binary_version" = "shprd $package_version" ] ||
  fail "binary version does not match package VERSION"

if { [ -e "$target" ] || [ -L "$target" ]; } &&
  { [ ! -f "$target" ] || [ -L "$target" ]; }; then
  fail "install target changed during installation"
fi
target_tmp="$(mktemp "$install_dir/.shprd.new.XXXXXX")" ||
  fail "cannot create temporary installed binary"
install -m 0755 "$binary" "$target_tmp"
backup=""
if [ -f "$target" ] && [ ! -L "$target" ]; then
  backup="$target.previous"
  backup_tmp="$(mktemp "$install_dir/.shprd.previous.XXXXXX")" ||
    fail "cannot create temporary backup binary"
  install -m 0755 "$target" "$backup_tmp"
  mv -f "$backup_tmp" "$backup"
  backup_tmp=""
fi
mv -f "$target_tmp" "$target"
target_tmp=""

if [ "$(id -u)" -eq 0 ]; then
  printf 'Installed SHPRD %s to %s\n' "$package_version" "$target"
  printf 'Run %s service install as the target user to create their persistent service.\n' \
    "$target"
else
  "$target" service install
  printf 'Installed SHPRD %s to %s and started its user service.\n' \
    "$package_version" "$target"
fi
if [ -n "$backup" ]; then
  printf 'Previous binary saved to %s\n' "$backup"
fi
case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *)
    printf 'Add %s to PATH to run shprd directly.\n' "$install_dir"
    ;;
esac
