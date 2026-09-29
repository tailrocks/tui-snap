#!/bin/sh
# Deterministic 3-row settings menu for the M2 PTY journey test.
# Keys: Up/Down move selection, Space toggles, q quits with exit = toggled count.
# No $RANDOM, no dates, no network: identical bytes on every run.

sel=0
t0=0
t1=0
t2=0

row_name() {
  case "$1" in
    0) printf 'autosave' ;;
    1) printf 'line_numbers' ;;
    2) printf 'word_wrap' ;;
  esac
}

draw() {
  printf '\033[2J\033[H\033[?25l'
  printf 'Settings (space toggles, q quits)\n\n'
  i=0
  while [ "$i" -lt 3 ]; do
    eval "v=\$t$i"
    if [ "$v" = "1" ]; then mark='[x]'; else mark='[ ]'; fi
    if [ "$i" = "$sel" ]; then
      printf '\033[7m> %s %s\033[0m\n' "$mark" "$(row_name "$i")"
    else
      printf '  %s %s\n' "$mark" "$(row_name "$i")"
    fi
    i=$((i + 1))
  done
}

restore() {
  stty sane 2>/dev/null
  printf '\033[?25h'
}

trap 'restore; exit 3' INT TERM HUP
stty -icanon -echo min 1 time 0 2>/dev/null
draw

while :; do
  code=$(dd bs=1 count=1 2>/dev/null | od -An -tu1 | tr -d ' \n')
  case "$code" in
    113) break ;; # q
    32) # space: toggle selected row
      eval "v=\$t$sel"
      if [ "$v" = "1" ]; then eval "t$sel=0"; else eval "t$sel=1"; fi
      draw
      ;;
    27) # ESC: expect '[' + final byte ("9165" Up, "9166" Down)
      seq=$(dd bs=2 count=1 2>/dev/null | od -An -tu1 | tr -d ' \n')
      case "$seq" in
        9165) sel=$(((sel + 2) % 3)); draw ;; # Up
        9166) sel=$(((sel + 1) % 3)); draw ;; # Down
      esac
      ;;
  esac
done

count=$((t0 + t1 + t2))
restore
exit "$count"
