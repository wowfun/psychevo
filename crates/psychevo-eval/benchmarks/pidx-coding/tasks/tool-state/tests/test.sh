expected='prepare
edit
verify'
actual="$(cat state.txt 2>/dev/null || true)"
test "$actual" = "$expected"
