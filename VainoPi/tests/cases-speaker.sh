# vaino-speaker: the policy of `[PI3-AIM-080]` as a truth table.
#
#   There is at most one speaker connected at a time. It is chosen when none
#   is connected -- the listener's speaker first, then any other known one --
#   and it is replaced only when it goes away.
group speaker || return 0
printf '\nspeaker\n'

keeper() { VAINO_TICK_SECONDS=4 VAINO_CHASE_SECONDS=2 sh "$PI/vaino-speaker.sh" 2>&1; }

# --- 1. nothing connected, the chosen speaker is reachable ----------------
# The regression of 2026-09-10 made this path unreachable: an `exit 0` above
# the chase meant nothing ever connected, and a tick with nothing to do looks
# exactly like a tick that did nothing.
setup
speaker "$OONTZ" "OontZ_Angle 3 U412" no yes
printf '%s\n' "$OONTZ" > "$VT_STATE/db_speaker"
printf '%s\n' "$OONTZ" > "$VT_STATE/reachable"
sinks "Dummy Output"
OUT=$(keeper)
assert_called "bluetoothctl connect $OONTZ" "chases the chosen speaker when nothing is connected"
assert_in "$OUT" "connected $OONTZ" "reports the connection"
teardown

# --- 2. chosen speaker absent, another known one available ----------------
setup
speaker "$OONTZ" "OontZ_Angle 3 U412" no yes
speaker "$MIDDL" "MIDDLETON" no no
printf '%s\n' "$OONTZ" > "$VT_STATE/db_speaker"
printf '%s\n' "$MIDDL" > "$VT_STATE/reachable"
sinks "Dummy Output"
OUT=$(keeper)
assert_called "bluetoothctl connect $OONTZ" "tries the listener's choice first"
assert_called "bluetoothctl connect $MIDDL" "falls back to another known speaker"
assert_in "$OUT" "is absent; trying known speaker" "says it is falling back"
teardown

# --- 3. two connected: the incumbent keeps the audio ----------------------
# The inversion of `[PI3-FOUND-640]`: an intruder must not win by breaking the
# incumbent, and must not be adopted because it is the listener's choice.
setup
speaker "$MIDDL" "MIDDLETON" yes yes
speaker "$OONTZ" "OontZ_Angle 3 U412" yes yes
printf '%s\n' "$MIDDL" > "$VAINO_RUN_DIR/incumbent"
printf '%s\n' "$OONTZ" > "$VT_STATE/db_speaker"
printf 'MIDDLETON\n' > "$VT_STATE/routed"
sinks "MIDDLETON" "OontZ_Angle 3 U412"
OUT=$(keeper)
assert_called "bluetoothctl disconnect $OONTZ" "disconnects the speaker that is not holding the audio"
assert_not_called "bluetoothctl disconnect $MIDDL" "never disconnects the incumbent"
assert_eq "$(cat "$VAINO_RUN_DIR/incumbent")" "$MIDDL" "incumbency stays with the holder"
assert_not_in "$OUT" "moved the stream" "does not move audio that is already playing"
teardown

# --- 4. incumbent connected but its sink has not appeared ----------------
# A missing sink is something to wait for, never a reason to switch: a reopen
# aimed at a sink that does not exist abandons working audio `[PI3-FOUND-590]`.
setup
speaker "$MIDDL" "MIDDLETON" yes yes
printf '%s\n' "$MIDDL" > "$VAINO_RUN_DIR/incumbent"
printf '%s\n' "$MIDDL" > "$VT_STATE/db_speaker"
: > "$VT_STATE/routed"
sinks "Dummy Output"
OUT=$(keeper)
assert_in "$OUT" "waiting, not switching" "waits for the sink instead of switching"
assert_not_called "command/reopen-output" "does not ask for a reopen it cannot satisfy"
teardown

# --- 5. trust follows the audio ------------------------------------------
# BlueZ consults an agent only for untrusted devices, so trust is the
# enforcement `[PI3-FOUND-680]`.
setup
speaker "$MIDDL" "MIDDLETON" yes no
speaker "$OONTZ" "OontZ_Angle 3 U412" no yes
printf '%s\n' "$MIDDL" > "$VAINO_RUN_DIR/incumbent"
printf '%s\n' "$OONTZ" > "$VT_STATE/db_speaker"
printf 'MIDDLETON\n' > "$VT_STATE/routed"
sinks "MIDDLETON"
keeper >/dev/null
assert_called "bluetoothctl trust $MIDDL" "trusts whoever holds the audio"
assert_called "bluetoothctl untrust $OONTZ" "untrusts everyone else, so the agent is reachable"
teardown

# --- 6. an unreadable database must not overwrite the choice -------------
# `[PI3-FOUND-700]`: "nobody chose" and "I could not ask" are different
# answers, and only one of them may be acted on.
setup
speaker "$MIDDL" "MIDDLETON" yes yes
printf '%s\n' "$MIDDL" > "$VAINO_RUN_DIR/incumbent"
printf 'MIDDLETON\n' > "$VT_STATE/routed"
sinks "MIDDLETON"
touch "$VT_STATE/db_fails"
OUT=$(keeper)
assert_eq "$(wc -l < "$VT_STATE/db_writes")" "0" "writes nothing when the database cannot be read"
assert_not_in "$OUT" "adopted" "does not adopt a speaker it could not check for"
teardown

# --- 7. no speaker ever chosen: adoption is allowed ----------------------
setup
speaker "$MIDDL" "MIDDLETON" yes yes
printf '%s\n' "$MIDDL" > "$VAINO_RUN_DIR/incumbent"
: > "$VT_STATE/db_speaker"
printf 'MIDDLETON\n' > "$VT_STATE/routed"
sinks "MIDDLETON"
OUT=$(keeper)
assert_in "$OUT" "adopted" "adopts when the database says nobody has chosen"
teardown

# --- 8. a tick must not outlast the timer that fires it ------------------
# `[PI3-FOUND-670]`: a tick longer than the period means the next one never
# starts, and the appliance stops reacting without failing.
setup
speaker "$OONTZ" "OontZ_Angle 3 U412" no yes
speaker "$MIDDL" "MIDDLETON" no no
printf '%s\n' "$OONTZ" > "$VT_STATE/db_speaker"
sinks "Dummy Output"
START=$(date +%s)
VAINO_TICK_SECONDS=6 VAINO_CHASE_SECONDS=3 sh "$PI/vaino-speaker.sh" >/dev/null 2>&1
ELAPSED=$(( $(date +%s) - START ))
if [ "$ELAPSED" -le 14 ]; then
    ok "a tick with nothing reachable stays inside its budget (${ELAPSED}s)"
else
    bad "a tick with nothing reachable stays inside its budget" "took ${ELAPSED}s"
fi
teardown
