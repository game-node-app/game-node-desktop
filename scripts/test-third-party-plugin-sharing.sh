#!/usr/bin/env bash
set -Eeuo pipefail

# Export this worktree's tracked changes as a compact patch, then apply that patch
# to an existing checkout on another machine and run only the related Rust tests.
readonly EXPECTED_BASE="903e9972f81bcad6df01eba5a7efdaa1a5c06ff0"
readonly CARGO_TARGET="/data/hdd1/arquivos/t3/cargo-targets/third-party-plugins"
readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"

usage() {
  cat <<'EOF'
Usage:
  scripts/test-third-party-plugin-sharing.sh
      Run the focused Rust tests in the current checkout.
  scripts/test-third-party-plugin-sharing.sh --export-patch FILE
      Export this worktree's tracked changes as a patch (run this here).
  scripts/test-third-party-plugin-sharing.sh --apply-patch FILE
      Apply the patch to an existing compatible checkout, then run tests.

Transfer only this script and the exported patch to the other machine; no worktree
copy is needed. The destination checkout must contain compatible integration code;
git apply --check guards against applying to an incompatible checkout. Cargo uses
the configured shared target and never falls back elsewhere.
EOF
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

export_patch() {
  local patch_file="$1"
  local head
  head="$(git -C "$REPO_ROOT" rev-parse HEAD)"
  [[ "$head" == "$EXPECTED_BASE" ]] || fail "expected base $EXPECTED_BASE, found $head"
  git -C "$REPO_ROOT" diff --quiet && fail "there are no tracked changes to export"
  mkdir -p -- "$(dirname -- "$patch_file")"
  git -C "$REPO_ROOT" diff --binary --no-ext-diff "$EXPECTED_BASE" -- > "$patch_file"
  [[ -s "$patch_file" ]] || fail "patch export produced an empty file"
  printf 'Patch exported: %s (%s bytes)\n' "$patch_file" "$(wc -c < "$patch_file" | tr -d ' ')"
}

apply_patch() {
  local patch_file="$1"
  [[ -f "$patch_file" ]] || fail "patch file not found: $patch_file"
  local head
  head="$(git -C "$REPO_ROOT" rev-parse HEAD)"
  printf 'Destination HEAD: %s\nPatch base: %s\n' "$head" "$EXPECTED_BASE"
  if git -C "$REPO_ROOT" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
    printf 'Patch is already applied.\n'
    return
  fi
  git -C "$REPO_ROOT" apply --check "$patch_file" || fail "patch does not apply cleanly; no files were changed"
  git -C "$REPO_ROOT" apply "$patch_file"
  printf 'Patch applied to %s\n' "$REPO_ROOT"
}

run_tests() {
  cd -- "$REPO_ROOT"

  command -v mountpoint >/dev/null 2>&1 || fail "mountpoint is required to verify the Cargo data mount"
  mountpoint -q /data/hdd1 || fail "/data/hdd1 is not mounted; refusing to use another Cargo target"
  mkdir -p -- "$CARGO_TARGET"

  local effective_target
  effective_target="$(CARGO_TARGET_DIR="$CARGO_TARGET" cargo metadata --no-deps --format-version 1 \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
  [[ "$effective_target" == "$CARGO_TARGET" ]] || fail "Cargo resolved target to $effective_target"
  printf 'Cargo target: %s\n' "$effective_target"

  local jobs="${CARGO_BUILD_JOBS:-1}"
  local filters=(
    'plugin_host::updates::tests::offline_license_reader_is_bounded_utf8_and_package_relative'
    'plugin_host::sharing_policy::tests::'
    'plugin_host::shared_data::tests::'
  )
  local filter
  for filter in "${filters[@]}"; do
    printf '\nRunning Rust test filter: %s\n' "$filter"
    CARGO_TARGET_DIR="$CARGO_TARGET" CARGO_BUILD_JOBS="$jobs" \
      cargo test --locked -p gamenode-app --lib "$filter"
  done
}

case "${1:-}" in
  --help|-h)
    usage
    ;;
  --export-patch)
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    export_patch "$2"
    ;;
  --apply-patch)
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    apply_patch "$2"
    run_tests
    ;;
  "")
    run_tests
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
