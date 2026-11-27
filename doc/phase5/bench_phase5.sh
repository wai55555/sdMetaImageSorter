#!/usr/bin/env bash
# Phase 5 の等価性・性能計測（§10.4）。
# 使い方: bash doc/phase5/bench_phase5.sh <old_exe> <new_exe> <corpus_dir_win> <runs>
set -u
OLD_EXE="$1"; NEW_EXE="$2"; CORPUS="$3"; RUNS="${4:-3}"
DEST_FOLDERS=(comfyui_img webui_img NovelAI_img SwarmUI_img InvokeAI_img)

map_destinations() {
  local dir="$1"
  ( cd "$dir" || exit 1
    for f in *; do
      [ -f "$f" ] || continue
      for dest in "${DEST_FOLDERS[@]}"; do
        if [ -f "$dest/$f" ]; then echo "$dest/$f"; fi
      done
    done | sort )
}

time_run() {
  local exe="$1" dir="$2"
  for dest in "${DEST_FOLDERS[@]}"; do rm -rf "$dir/$dest"; done
  local start end
  start=$(date +%s%N)
  "$exe" "$(wslpath -w "$dir")" --copy >/dev/null 2>"$dir.stderr"
  end=$(date +%s%N)
  echo $(( (end - start) / 1000000 ))
}

for label in old new; do
  exe="$OLD_EXE"; [ "$label" = new ] && exe="$NEW_EXE"
  dir="$(dirname "$CORPUS")/bench-$label"
  rm -rf "$dir"; cp -r "$CORPUS" "$dir"
  times=()
  for i in $(seq 1 "$RUNS"); do times+=("$(time_run "$exe" "$dir")"); done
  map_destinations "$dir" > "$dir.map"
  echo "$label times_ms: ${times[*]}"
  echo "$label stderr_lines: $(wc -l < "$dir.stderr")"
done

echo "--- destination diff (old vs new) ---"
diff "bench-old.map" "bench-new.map" 2>/dev/null || \
  diff "$(dirname "$CORPUS")/bench-old.map" "$(dirname "$CORPUS")/bench-new.map"
