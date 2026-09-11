# vaino-btctl: the verbs the settings panel calls. This is the surface the
# listener actually touches, it writes `speaker_address`, and its address
# argument is the only untrusted input this appliance takes -- it arrives from
# a browser.
group btctl || return 0
printf '\nbtctl\n'

btctl() { VAINO_DB="$VT_STATE/fake.db" bash "$PI/vaino-btctl" "$@" 2>&1; }

# --- the untrusted input ------------------------------------------------
# Shape-checked before it reaches bluetoothctl or SQL. Every one of these
# would be harmless on its own and none should reach a command line.
setup
sinks "MIDDLETON"
for bad_addr in "not-an-address" "20:64:DE:CF:F3" "20:64:DE:CF:F3:AD:99" \
                "\$(touch $VT_STATE/pwned)" "20:64:DE:CF:F3:AD; rm -rf /" ""; do
    OUT=$(btctl use "$bad_addr")
    case "$OUT" in
        *"not a device address"*) ok "rejects a malformed address: ${bad_addr:-<empty>}" ;;
        *) bad "rejects a malformed address: ${bad_addr:-<empty>}" "got: $OUT" ;;
    esac
done
if [ -f "$VT_STATE/pwned" ]; then
    bad "a command substitution in the address is never evaluated" "it ran"
else
    ok "a command substitution in the address is never evaluated"
fi
assert_not_called "bluetoothctl connect not-an-address" "never passes a rejected address to bluetoothctl"
teardown

# --- use: connect, trust, and record the choice -------------------------
setup
speaker "$MIDDL" "MIDDLETON" no no
printf '%s\n' "$MIDDL" > "$VT_STATE/reachable"
sinks "MIDDLETON"
btctl use "$MIDDL" >/dev/null
assert_called "bluetoothctl trust $MIDDL" "use trusts the chosen speaker, so it can reconnect after a power cut"
assert_called "bluetoothctl connect $MIDDL" "use connects it"
teardown

# --- forget: the recovery path the listener reaches for -----------------
setup
speaker "$MIDDL" "MIDDLETON" yes yes
sinks "MIDDLETON"
btctl forget "$MIDDL" >/dev/null
assert_called "bluetoothctl remove $MIDDL" "forget removes the pairing"
teardown

# --- status answers without touching the radio --------------------------
# The settings panel polls this; it must not page anything.
setup
speaker "$MIDDL" "MIDDLETON" yes yes
sinks "MIDDLETON"
OUT=$(btctl status "$MIDDL")
assert_in "$OUT" "{" "status answers in JSON"
assert_not_called "bluetoothctl connect" "status never pages a device"
teardown
