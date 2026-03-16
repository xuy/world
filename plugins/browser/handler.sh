#!/bin/sh
# Browser domain handler for world.
#
# Protocol: reads a JSON request from stdin, writes a JSON response to stdout.
# Delegates to agent-browser CLI, which manages browser state via its daemon.
#
# Session domain (session: true) — observe returns nulls when no page is open,
# and the "open" action populates the observation space.

set -e

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.command')
HANDLER=$(echo "$INPUT" | jq -r '.handler // empty')
TARGET=$(echo "$INPUT" | jq -r '.target // empty')
DRY_RUN=$(echo "$INPUT" | jq -r '.dry_run // false')

# ─── Helpers ────────────────────────────────────────────────────────────────

# Take a snapshot and return structured observations.
snapshot() {
    SNAP_FILE=$(mktemp)
    trap "rm -f $SNAP_FILE" EXIT

    agent-browser snapshot --json -i -c > "$SNAP_FILE" 2>/dev/null || true

    ORIGIN=$(jq -r '.data.origin // empty' < "$SNAP_FILE")
    if [ ! -s "$SNAP_FILE" ] || [ -z "$ORIGIN" ] || [ "$ORIGIN" = "about:blank" ]; then
        rm -f "$SNAP_FILE"
        echo '{"details":{"url":null,"title":null,"elements":[],"snapshot":null}}'
        return
    fi

    # Get title via eval (may fail if page has no title); strip surrounding quotes
    TITLE=$(agent-browser eval 'document.title' 2>/dev/null | sed 's/^"//;s/"$//') || TITLE=""

    # Build the full response from the snapshot JSON, adding title
    jq --arg title "$TITLE" '{
        details: {
            url: .data.origin,
            title: (if $title == "" then null else $title end),
            elements: ([.data.refs | to_entries[] | {ref: .key, role: .value.role, name: .value.name}] | sort_by(.ref)),
            snapshot: .data.snapshot
        }
    }' < "$SNAP_FILE"
    rm -f "$SNAP_FILE"
}

# Run an agent-browser command; on failure return an error JSON.
run_ab() {
    OUTPUT=$(agent-browser "$@" 2>&1) || {
        echo "$OUTPUT" | jq -Rs '{error: {code: "browser_error", message: .}}'
        return 1
    }
    return 0
}

# ─── Observe ────────────────────────────────────────────────────────────────

if [ "$COMMAND" = "observe" ]; then
    snapshot
    exit 0
fi

# ─── Act ────────────────────────────────────────────────────────────────────

if [ "$COMMAND" = "act" ]; then

    if [ "$DRY_RUN" = "true" ]; then
        case "$HANDLER" in
            navigate)
                URL=$(echo "$INPUT" | jq -r '.params.url // empty')
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser open $URL\"}}"
                ;;
            close)
                echo '{"details":{"dry_run":true,"would_run":"agent-browser close"}}'
                ;;
            click)
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser click $TARGET\"}}"
                ;;
            fill)
                TEXT=$(echo "$INPUT" | jq -r '.params.text // empty')
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser fill $TARGET '$TEXT'\"}}"
                ;;
            select)
                VAL=$(echo "$INPUT" | jq -r '.params.value // empty')
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser select $TARGET '$VAL'\"}}"
                ;;
            hover)
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser hover $TARGET\"}}"
                ;;
            scroll)
                DIR=$(echo "$INPUT" | jq -r '.params.direction // "down"')
                PX=$(echo "$INPUT" | jq -r '.params.pixels // 300')
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser scroll $DIR $PX\"}}"
                ;;
            press_key)
                KEY=$(echo "$INPUT" | jq -r '.params.key // empty')
                echo "{\"details\":{\"dry_run\":true,\"would_run\":\"agent-browser press $KEY\"}}"
                ;;
            eval_js)
                echo '{"details":{"dry_run":true,"would_run":"agent-browser eval <js>"}}'
                ;;
            *)
                echo "{\"error\":{\"code\":\"unknown_handler\",\"message\":\"Unknown handler: $HANDLER\"}}"
                ;;
        esac
        exit 0
    fi

    case "$HANDLER" in
        navigate)
            URL=$(echo "$INPUT" | jq -r '.params.url // empty')
            if [ -z "$URL" ]; then
                echo '{"error":{"code":"missing_param","message":"url= required for open"}}'
                exit 0
            fi
            run_ab open "$URL" || exit 0
            snapshot
            ;;
        close)
            run_ab close || exit 0
            echo '{"details":{"url":null,"title":null,"elements":[],"snapshot":null}}'
            ;;
        click)
            if [ -z "$TARGET" ]; then
                echo '{"error":{"code":"missing_target","message":"element ref required for click"}}'
                exit 0
            fi
            run_ab click "$TARGET" || exit 0
            snapshot
            ;;
        fill)
            TEXT=$(echo "$INPUT" | jq -r '.params.text // empty')
            if [ -z "$TARGET" ]; then
                echo '{"error":{"code":"missing_target","message":"element ref required for fill"}}'
                exit 0
            fi
            run_ab fill "$TARGET" "$TEXT" || exit 0
            snapshot
            ;;
        select)
            VAL=$(echo "$INPUT" | jq -r '.params.value // empty')
            if [ -z "$TARGET" ]; then
                echo '{"error":{"code":"missing_target","message":"element ref required for select"}}'
                exit 0
            fi
            run_ab select "$TARGET" "$VAL" || exit 0
            snapshot
            ;;
        hover)
            if [ -z "$TARGET" ]; then
                echo '{"error":{"code":"missing_target","message":"element ref required for hover"}}'
                exit 0
            fi
            run_ab hover "$TARGET" || exit 0
            snapshot
            ;;
        scroll)
            DIR=$(echo "$INPUT" | jq -r '.params.direction // "down"')
            PX=$(echo "$INPUT" | jq -r '.params.pixels // 300')
            run_ab scroll "$DIR" "$PX" || exit 0
            snapshot
            ;;
        press_key)
            KEY=$(echo "$INPUT" | jq -r '.params.key // empty')
            if [ -z "$KEY" ]; then
                echo '{"error":{"code":"missing_param","message":"key= required for press"}}'
                exit 0
            fi
            run_ab press "$KEY" || exit 0
            snapshot
            ;;
        eval_js)
            JS=$(echo "$INPUT" | jq -r '.params.js // empty')
            if [ -z "$JS" ]; then
                echo '{"error":{"code":"missing_param","message":"js= required for eval"}}'
                exit 0
            fi
            RESULT=$(agent-browser eval "$JS" 2>&1) || {
                echo "$RESULT" | jq -Rs '{error: {code: "eval_error", message: .}}'
                exit 0
            }
            echo "$RESULT" | jq -Rs '{details: {result: .}}'
            ;;
        *)
            echo "{\"error\":{\"code\":\"unknown_handler\",\"message\":\"Unknown handler: $HANDLER\"}}"
            ;;
    esac
    exit 0
fi

echo '{"error":{"code":"unknown_command","message":"Unknown command: '"$COMMAND"'"}}'
