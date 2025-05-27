#!/bin/bash
set -eux
# Patch pg_hba.conf to allow replication from any host
PG_HBA="$PGDATA/pg_hba.conf"
PG_CONF="$PGDATA/postgresql.conf"
echo "host replication all 0.0.0.0/0 trust" >> "$PG_HBA"
echo "host all all 0.0.0.0/0 trust" >> "$PG_HBA"
echo "wal_level = replica" >> "$PG_CONF"
echo "max_wal_senders = 10" >> "$PG_CONF"
echo "listen_addresses = '*'" >> "$PG_CONF"
