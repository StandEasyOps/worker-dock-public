#!/bin/sh
# Prove the Debian postinst never signals every user's worker-dock GUI.
# Uses WORKER_DOCK_EUID_OVERRIDE / WORKER_DOCK_PKILL so CI (non-root) can
# exercise the root maintainer-script branches without touching real processes.
set -eu

POSTINST="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/postinst"
[ -f "$POSTINST" ] || {
  echo "missing $POSTINST" >&2
  exit 1
}

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

pass() {
  echo "ok: $*"
}

# --- static: no machine-wide GUI kill, no service restart ---------------

if grep -E '^[[:space:]]*pkill[[:space:]]+-x[[:space:]]+worker-dock([[:space:]]|$)' "$POSTINST" >/dev/null; then
  fail "global 'pkill -x worker-dock' is forbidden; must target a uid"
fi
if ! grep -F 'pkill -u "$uid" -x worker-dock' "$POSTINST" >/dev/null; then
  fail "expected per-uid pkill -u \"\$uid\" -x worker-dock"
fi
if grep -E '^[[:space:]]*(systemctl|pkill).*worker-dock-run' "$POSTINST" >/dev/null; then
  fail "must not stop or restart worker-dock-run"
fi
if grep -E '^[[:space:]]*systemctl([[:space:]]|.*--user)' "$POSTINST" >/dev/null; then
  fail "must not restart systemd units from the root maintainer script"
fi
pass "static: per-uid pkill only; no worker-dock-run / systemctl"

# --- sourced desktop_uid -----------------------------------------------

WORKER_DOCK_SOURCED=1
# shellcheck disable=SC1090
. "$POSTINST"

got="$(
  WORKER_DOCK_EUID_OVERRIDE=1000
  unset SUDO_UID PKEXEC_UID SUDO_USER
  desktop_uid
)"
[ "$got" = 1000 ] || fail "non-root euid should win, got '$got'"
pass "desktop_uid: non-root euid"

got="$(
  WORKER_DOCK_EUID_OVERRIDE=1000
  SUDO_UID=1001
  PKEXEC_UID=1002
  SUDO_USER=someone
  desktop_uid
)"
[ "$got" = 1000 ] || fail "non-root euid must ignore SUDO_*/PKEXEC_*, got '$got'"
pass "desktop_uid: planted SUDO_UID cannot retarget a non-root script"

got="$(
  WORKER_DOCK_EUID_OVERRIDE=0
  SUDO_UID=1001
  unset PKEXEC_UID SUDO_USER
  desktop_uid
)"
[ "$got" = 1001 ] || fail "root + SUDO_UID, got '$got'"
pass "desktop_uid: SUDO_UID"

got="$(
  WORKER_DOCK_EUID_OVERRIDE=0
  unset SUDO_UID SUDO_USER
  PKEXEC_UID=1002
  desktop_uid
)"
[ "$got" = 1002 ] || fail "root + PKEXEC_UID, got '$got'"
pass "desktop_uid: PKEXEC_UID"

got="$(
  WORKER_DOCK_EUID_OVERRIDE=0
  SUDO_UID=1001
  PKEXEC_UID=1002
  desktop_uid
)"
[ "$got" = 1001 ] || fail "SUDO_UID should beat PKEXEC_UID, got '$got'"
pass "desktop_uid: SUDO_UID preferred over PKEXEC_UID"

if ! (
  WORKER_DOCK_EUID_OVERRIDE=0
  unset SUDO_UID PKEXEC_UID SUDO_USER
  desktop_uid
); then
  pass "desktop_uid: unidentified root is empty"
else
  fail "unidentified root should not yield a uid"
fi

if ! (
  WORKER_DOCK_EUID_OVERRIDE=0
  unset SUDO_UID PKEXEC_UID
  SUDO_UID=0
  SUDO_USER=root
  desktop_uid
); then
  pass "desktop_uid: never uid 0 / root"
else
  fail "uid 0 / SUDO_USER=root must be rejected"
fi

if ! (
  WORKER_DOCK_EUID_OVERRIDE=0
  unset SUDO_UID PKEXEC_UID
  SUDO_USER='alice;rm -rf /'
  desktop_uid
); then
  pass "desktop_uid: hostile SUDO_USER rejected"
else
  fail "metacharacters in SUDO_USER must be rejected"
fi

me="$(id -un)"
my_uid="$(id -u)"
case "$me" in
  ''|root|*[!A-Za-z0-9._-]*)
    pass "desktop_uid: skip SUDO_USER resolve (unsafe local name '$me')"
    ;;
  *)
    got="$(
      WORKER_DOCK_EUID_OVERRIDE=0
      unset SUDO_UID PKEXEC_UID
      SUDO_USER="$me"
      desktop_uid
    )"
    [ "$got" = "$my_uid" ] || fail "SUDO_USER=$me should resolve to $my_uid, got '$got'"
    pass "desktop_uid: sanitized SUDO_USER -> uid"
    ;;
esac

# --- subprocess configure with mock pkill ------------------------------

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
MOCK="$WORKDIR/mock-pkill"
LOG="$WORKDIR/pkill.log"
NOTE="$WORKDIR/stderr.log"

cat > "$MOCK" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "${WORKER_DOCK_PKILL_LOG:?}"
EOF
chmod +x "$MOCK"

run_postinst() {
  action="$1"
  : > "$LOG"
  : > "$NOTE"
  WORKER_DOCK_PKILL="$MOCK" \
    WORKER_DOCK_PKILL_LOG="$LOG" \
    WORKER_DOCK_EUID_OVERRIDE="${WORKER_DOCK_EUID_OVERRIDE-}" \
    SUDO_UID="${SUDO_UID-}" \
    PKEXEC_UID="${PKEXEC_UID-}" \
    SUDO_USER="${SUDO_USER-}" \
    sh "$POSTINST" "$action" 2>"$NOTE" || fail "postinst $action exited $?"
}

expect_pkill() {
  expected="$1"
  got="$(cat "$LOG")"
  [ "$got" = "$expected" ] || fail "pkill argv: got '$got' want '$expected'"
  case "$got" in
    *-x\ worker-dock-run*|*-x\ worker-dock\ *)
      fail "pkill must be exact comm worker-dock only: $got"
      ;;
  esac
}

expect_no_pkill() {
  if [ -s "$LOG" ]; then
    fail "expected no pkill, got: $(cat "$LOG")"
  fi
}

WORKER_DOCK_EUID_OVERRIDE=1000
unset SUDO_UID PKEXEC_UID SUDO_USER || true
SUDO_UID=
PKEXEC_UID=
SUDO_USER=
run_postinst configure
expect_pkill "-u 1000 -x worker-dock"
pass "configure: non-root euid -> pkill -u 1000"

WORKER_DOCK_EUID_OVERRIDE=0
SUDO_UID=1001
PKEXEC_UID=
SUDO_USER=
run_postinst configure
expect_pkill "-u 1001 -x worker-dock"
pass "configure: root + SUDO_UID -> pkill -u 1001 only"

WORKER_DOCK_EUID_OVERRIDE=0
SUDO_UID=
PKEXEC_UID=1002
SUDO_USER=
run_postinst configure
expect_pkill "-u 1002 -x worker-dock"
pass "configure: root + PKEXEC_UID -> pkill -u 1002 only"

WORKER_DOCK_EUID_OVERRIDE=0
SUDO_UID=
PKEXEC_UID=
SUDO_USER=
run_postinst configure
expect_no_pkill
grep -q 'left running windows alone' "$NOTE" || fail "missing skip note on stderr: $(cat "$NOTE")"
pass "configure: unidentified root skips GUI kill and logs a note"

WORKER_DOCK_EUID_OVERRIDE=0
SUDO_UID=0
PKEXEC_UID=
SUDO_USER=root
run_postinst configure
expect_no_pkill
grep -q 'left running windows alone' "$NOTE" || fail "uid 0 must skip GUI kill"
pass "configure: uid 0 / root is not a desktop user"

WORKER_DOCK_EUID_OVERRIDE=0
SUDO_UID=
PKEXEC_UID=
SUDO_USER='alice;rm -rf /'
run_postinst configure
expect_no_pkill
pass "configure: hostile SUDO_USER skips GUI kill"

WORKER_DOCK_EUID_OVERRIDE=1000
SUDO_UID=1001
PKEXEC_UID=
SUDO_USER=
run_postinst abort-upgrade
expect_no_pkill
pass "non-configure action does not signal anyone"

echo "all postinst targeting checks passed"
