#!/usr/bin/env bash
# Prepare a clean, pre-seeded store for the VHS demo (demos/demo.tape).
#
# It recreates the demo database from demos/demo.sql, then registers the local
# instance and one connection in a throwaway DBM_DATA_DIR so the TUI has
# something to show as soon as it starts.
#
# Overridable environment variables:
#   DEMO_PORT      port the demo PostgreSQL listens on   (default: 5482)
#   DEMO_PG_URL    admin URL for (re)creating the demo DB
#                  (default: postgres://$(whoami)@127.0.0.1:$DEMO_PORT/postgres)
#   DEMO_PG_USER   connection username stored in dbm     (default: $(whoami))
#   DEMO_DB        demo database name                    (default: dbm_demo)
#   DEMO_INSTANCE  registered instance name              (default: pg16)
#   DEMO_DATA_DIR  store directory used while recording  (default: /tmp/dbm-demo-data)
#   DBM            path to the dbm binary                (default: ./target/release/dbm)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")

PORT="${DEMO_PORT:-5482}"
PGUSER="${DEMO_PG_USER:-$(whoami)}"
DB="${DEMO_DB:-dbm_demo}"
INSTANCE="${DEMO_INSTANCE:-pg16}"
DATA_DIR="${DEMO_DATA_DIR:-/tmp/dbm-demo-data}"
DBM="${DBM:-$root/target/release/dbm}"
PGURL="${DEMO_PG_URL:-postgres://${PGUSER}@127.0.0.1:${PORT}/postgres}"

[ -x "$DBM" ] || { echo "dbm binary not found at $DBM — run: cargo build --release -p dbm-cli" >&2; exit 1; }

echo "-> recreating database ${DB}"
psql "$PGURL" -c "DROP DATABASE IF EXISTS ${DB}" >/dev/null
psql "$PGURL" -c "CREATE DATABASE ${DB}" >/dev/null
psql "${PGURL%/*}/${DB}" -q -f "$here/demo.sql"

echo "-> resetting demo store at ${DATA_DIR}"
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"
export DBM_DATA_DIR="$DATA_DIR"

echo "-> scanning 127.0.0.1:${PORT}"
"$DBM" discover scan --ports "$PORT" >/dev/null

id=$("$DBM" discover list | awk -v p="127.0.0.1:${PORT}" '$3 == p { print $2; exit }')
[ -n "$id" ] || { echo "no discovered instance on 127.0.0.1:${PORT}" >&2; exit 1; }

echo "-> registering ${INSTANCE}"
"$DBM" instance register "$id" --name "$INSTANCE" --force >/dev/null

echo "-> adding connection demo (${PGUSER}@${DB})"
"$DBM" instance connection add \
    --instance "$INSTANCE" --name demo \
    --user "$PGUSER" --database "$DB" --ssl-mode disable >/dev/null

echo "ready: DBM_DATA_DIR=${DATA_DIR}"
