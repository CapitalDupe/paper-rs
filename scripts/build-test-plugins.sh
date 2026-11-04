#!/usr/bin/env bash
# Builds the Java test plugins in test-plugins/* into target/test-plugins/*.jar.
# Needs a JDK (javac and jar) on PATH. Used by the paper-jvm and paper-plugin tests.
set -euo pipefail
cd "$(dirname "$0")/.."
out="target/test-plugins"
rm -rf "$out"
mkdir -p "$out"
for dir in test-plugins/*/; do
  name="$(basename "$dir")"
  classes="$out/$name-classes"
  mkdir -p "$classes"
  javac -d "$classes" $(find "$dir/src" -name '*.java')
  cp "$dir/plugin.yml" "$classes/plugin.yml"
  jar cf "$out/$name.jar" -C "$classes" .
  echo "built $out/$name.jar"
done
