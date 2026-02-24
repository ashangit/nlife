#!/usr/bin/env bash
# PostToolUse hook: fires after any Edit or Write on a Rust source file or
# Cargo manifest.
#   1. Builds the project (fast failure on compile errors).
#   2. Runs the full test suite.
#   3. Reminds Claude to keep README.md and CLAUDE.md in sync.
#
# Receives a JSON object on stdin with tool_input.file_path.
# Exits 0 in all cases (non-blocking); stdout is shown to Claude.

INPUT=$(cat)
FILE_PATH=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null)

# Nothing to do if we can't determine the path.
[[ -z "$FILE_PATH" ]] && exit 0

# Don't trigger when editing the doc files themselves.
BASE=$(basename "$FILE_PATH")
[[ "$BASE" == "README.md" || "$BASE" == "CLAUDE.md" ]] && exit 0

# Trigger only for Rust source files and Cargo manifests.
if [[ "$FILE_PATH" == *.rs || "$BASE" == "Cargo.toml" ]]; then
    # Walk up to the workspace root (where Cargo.toml lives).
    DIR="$(dirname "$FILE_PATH")"
    while [[ ! -f "$DIR/Cargo.toml" && "$DIR" != "/" ]]; do
        DIR="$(dirname "$DIR")"
    done

    echo "--- build after '$BASE' changed ---"
    cargo build --manifest-path "$DIR/Cargo.toml" 2>&1
    BUILD_EXIT=$?

    if [[ $BUILD_EXIT -eq 0 ]]; then
        echo "--- tests ---"
        cargo test --quiet --manifest-path "$DIR/Cargo.toml" 2>&1
        TEST_EXIT=$?
    else
        echo "--- build failed; skipping tests ---"
        TEST_EXIT=1
    fi

    echo "--- docs reminder ---"
    echo "Update README.md if features/shortcuts/build steps changed."
    echo "Update CLAUDE.md if modules, the frame loop, or invariants changed."

    if [[ $BUILD_EXIT -eq 0 && $TEST_EXIT -eq 0 ]]; then
        echo "--- commit ---"
        echo "Build and tests passed. Propose a git commit to the user if the change is complete."
    fi
fi

exit 0
