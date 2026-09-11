#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_dir="${project_dir}/.build"
stormlib_dir="$(brew --prefix stormlib)"
compiler="$(xcrun --find clang++)"
sdk_dir="$(xcrun --show-sdk-path)"

mkdir -p "${build_dir}"
"${compiler}" \
  -std=c++17 \
  -O2 \
  -Wall \
  -Wextra \
  -Wpedantic \
  -isysroot "${sdk_dir}" \
  -I"${stormlib_dir}/include" \
  -L"${stormlib_dir}/lib" \
  -Wl,-rpath,"${stormlib_dir}/lib" \
  "${project_dir}/tools/lom_mpq.cpp" \
  -lstorm \
  -o "${build_dir}/lom-mpq"

echo "Built ${build_dir}/lom-mpq"
