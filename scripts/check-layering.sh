#!/usr/bin/env bash
# Architecture rule 1: engine crates must not depend, even transitively, on
# UI, windowing or GPU-surface crates. Add each new engine crate to ENGINE.
set -euo pipefail

ENGINE="iw-engine"
FORBIDDEN='^(winit|wgpu|wgpu-core|wgpu-hal|egui|eframe|egui_dock|egui-wgpu|egui-winit|muda|rfd|raw-window-handle) '

status=0
for crate in $ENGINE; do
  # Separate assignment so a cargo failure aborts instead of reading as "clean".
  tree=$(cargo tree -p "$crate" -e normal,build --prefix none --target all)
  hits=$(echo "$tree" | sort -u | grep -E "$FORBIDDEN" || true)
  if [ -n "$hits" ]; then
    echo "layering violation: $crate depends on:"
    echo "$hits"
    status=1
  fi
done
[ "$status" -eq 0 ] && echo "layering ok: $ENGINE"
exit "$status"
