#!/usr/bin/env bash
# Prepare the demo database for the recording (demos/record.sh).
#
# It recreates the demo database from demos/demo.sql and wipes the demo store,
# which is deliberately left *empty*: the recording registers the local
# instance and adds the connection from inside the TUI, so the demo shows the
# whole journey (discover -> register -> connect -> query).
#
# Overridable environment variables:
#   DEMO_PORT      port the demo PostgreSQL listens on   (default: 5482)
#   DEMO_PG_URL    admin URL for (re)creating the demo DB
#                  (default: postgres://$(whoami)@127.0.0.1:$DEMO_PORT/postgres)
#   DEMO_DB        demo database name                    (default: dbm_demo)
#   DEMO_DATA_DIR  store directory used while recording  (default: /tmp/dbm-demo-data)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

PORT="${DEMO_PORT:-5482}"
PGUSER="${DEMO_PG_USER:-$(whoami)}"
DB="${DEMO_DB:-dbm_demo}"
DATA_DIR="${DEMO_DATA_DIR:-/tmp/dbm-demo-data}"
PGURL="${DEMO_PG_URL:-postgres://${PGUSER}@127.0.0.1:${PORT}/postgres}"

echo "-> recreating database ${DB}"
psql "$PGURL" -c "DROP DATABASE IF EXISTS ${DB}" >/dev/null
psql "$PGURL" -c "CREATE DATABASE ${DB}" >/dev/null
psql "${PGURL%/*}/${DB}" -q -f "$here/demo.sql"

echo "-> resetting demo store at ${DATA_DIR} (left empty on purpose)"
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"

echo "ready: DBM_DATA_DIR=${DATA_DIR} (no instance registered yet)"
