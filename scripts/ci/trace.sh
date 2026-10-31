#!/usr/bin/env bash
# trace.sh — phase markers and a heartbeat for long CI steps.
#
# GitHub does not serve logs for a job that is still running, so a slow step is
# indistinguishable from a hung one until it ends. Sourcing this file gives a
# step timestamped markers around each phase (so the post-mortem shows where the
# time went) and a background heartbeat (so the live log keeps moving while a
# build is quiet, which is what makes "still working" versus "stuck" visible in
# the UI).
#
# Usage:
#
#   source scripts/ci/trace.sh
#   trap trace_heartbeat_stop EXIT
#   trace_mark "building universal app and dmg"
#   trace_heartbeat_start 60
#   pnpm tauri build --verbose ...
#
# Markers go to stdout as plain lines (no `::notice::`): annotations would spam
# the run summary, and these are diagnostic, not user-facing.

trace_mark() {
    printf '[trace] %s %s\n' "$(date -u +%H:%M:%SZ)" "$*"
}

trace_heartbeat_start() {
    local interval="${1:-60}"
    # Subshell in the background: prints while nothing else does, then dies with
    # the step (the EXIT trap) or via trace_heartbeat_stop.
    (
        while true; do
            sleep "${interval}"
            printf '[trace] %s still running\n' "$(date -u +%H:%M:%SZ)"
        done
    ) &
    TRACE_HEARTBEAT_PID=$!
}

trace_heartbeat_stop() {
    if [ -n "${TRACE_HEARTBEAT_PID:-}" ]; then
        kill "${TRACE_HEARTBEAT_PID}" 2>/dev/null || true
        wait "${TRACE_HEARTBEAT_PID}" 2>/dev/null || true
        TRACE_HEARTBEAT_PID=
    fi
}

# trace_phase <label> <command...> — marks, times, and runs one phase.
trace_phase() {
    local label="$1"
    shift
    trace_mark "${label} started"
    local started at
    started=${SECONDS}
    "$@"
    at=$((SECONDS - started))
    trace_mark "${label} finished in ${at}s"
}
