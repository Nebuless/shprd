#!/bin/sh
set -eu

# shellcheck disable=SC1091
. ./crates/shprd-shell/android-env.sh

bun run build:web
dx build --package shprd-shell --platform android --release --target aarch64-linux-android --no-default-features --features mobile

android_project="target/dx/shprd-shell/release/android/app"
android_assets="$android_project/app/src/main/assets"
test -d "$android_project/app/src/main"
rm -rf "$android_assets"
mkdir -p "$android_assets"
cp -R server/public/assets/. "$android_assets"
find server/public -mindepth 1 -maxdepth 1 -type f -exec cp {} "$android_assets" \;

(
  cd "$android_project"
  ./gradlew --no-daemon \
    -Djava.io.tmpdir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}" \
    -Dorg.gradle.internal.http.connectionTimeout=120000 \
    -Dorg.gradle.internal.http.socketTimeout=120000 \
    :app:packageRelease
)
