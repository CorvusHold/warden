#!/bin/sh
set -euo pipefail

PG_HBA="${PGDATA}/pg_hba.conf"
PG_CONF="${PGDATA}/postgresql.conf"

# Allow replication and client connections from any host for tests
{
  echo "host    all             all             0.0.0.0/0               trust"
  echo "host    all             all             ::/0                    trust"
  echo "host    replication     all             0.0.0.0/0               trust"
  echo "host    replication     all             ::/0                    trust"
} >>"${PG_HBA}"

# Ensure WAL configuration supports basebackup operations
{
  echo "wal_level = replica"
  echo "max_wal_senders = 10"
  echo "archive_mode = off"
} >>"${PG_CONF}"
