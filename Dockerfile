# Warden PostgreSQL Data Protection Agent
# Multi-stage build for minimal production image
#
# Build: docker build -t warden:latest .
# Run:   docker run -v /path/to/config:/etc/warden warden:latest

# =============================================================================
# Stage 0: Build Rust binary
# =============================================================================
FROM rust:1.82-bookworm AS builder

WORKDIR /usr/src/warden

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        clang \
        cmake \
        make && \
    rm -rf /var/lib/apt/lists/*

# Copy sources (context trimmed by .dockerignore) and build release binary
COPY . .
RUN cargo fetch --locked && \
    cargo build --locked --release -p warden

# =============================================================================
# Stage 1: Base image with PostgreSQL client tools
# =============================================================================
FROM ubuntu:24.04 AS base

# Set build arguments for versions
ARG PG_VERSION=17
ARG DEBIAN_FRONTEND=noninteractive

# Ensure noninteractive front-end for all apt operations
ENV DEBIAN_FRONTEND=${DEBIAN_FRONTEND} \
    DEBCONF_NONINTERACTIVE_SEEN=true

# Install PostgreSQL client tools and minimal dependencies
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        postgresql-common \
        libssl3 \
        curl \
        gnupg \
        lsb-release && \
    yes | sh /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh && \
    apt-get update && \
    apt-get install -y --no-install-recommends postgresql-client-${PG_VERSION} && \
    # Security: Remove package manager caches and temporary files
    apt-get clean && \
    rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/* /var/cache/apt/archives/* && \
    # Remove unnecessary packages to reduce attack surface
    apt-get autoremove -y && \
    # Verify PostgreSQL client installation
    pg_dump --version

# =============================================================================
# Stage 2: Production image
# =============================================================================
FROM base AS production

# Labels for container metadata (OCI standard)
LABEL org.opencontainers.image.title="Warden" \
      org.opencontainers.image.description="PostgreSQL Data Protection Agent" \
      org.opencontainers.image.vendor="Corvus" \
      org.opencontainers.image.source="https://github.com/corvushold/warden" \
      org.opencontainers.image.licenses="FSL-1.1-ALv2" \
      org.opencontainers.image.documentation="https://github.com/corvushold/warden/blob/main/README.md" \
      org.opencontainers.image.created="" \
      org.opencontainers.image.version=""

# Environment configuration
ENV APP_USER=warden \
    APP_GROUP=warden \
    APP_UID=1000 \
    APP_GID=1000 \
    WARDEN_CONFIG=/etc/warden/config.yaml \
    WARDEN_CLUSTER_CONFIG=/etc/warden/cluster.yaml \
    WARDEN_LOG_LEVEL=info \
    WARDEN_METRICS_PORT=9090 \
    PATH="/usr/local/bin:${PATH}"

# Create non-root user with specific UID/GID for consistent permissions
# Handle cases where the UID/GID may already exist in the base image
RUN set -eux; \
    # Resolve group: prefer existing group with target GID, else create; fallback to APP_GROUP name
    EXISTING_GROUP="$(getent group "${APP_GID}" | cut -d: -f1 || true)"; \
    TARGET_GROUP="${EXISTING_GROUP:-${APP_GROUP}}"; \
    if ! getent group "${TARGET_GROUP}" >/dev/null; then \
        groupadd -r -g "${APP_GID}" "${TARGET_GROUP}"; \
    fi; \
    # Resolve user: prefer requested UID; if UID is taken by another user, create without forcing UID
    UID_TAKEN_BY="$(getent passwd "${APP_UID}" | cut -d: -f1 || true)"; \
    if getent passwd "${APP_USER}" >/dev/null; then \
        if [ -z "${UID_TAKEN_BY}" ] || [ "${UID_TAKEN_BY}" = "${APP_USER}" ]; then \
            usermod -u "${APP_UID}" -g "${TARGET_GROUP}" "${APP_USER}"; \
        else \
            usermod -g "${TARGET_GROUP}" "${APP_USER}"; \
        fi; \
    else \
        if [ -z "${UID_TAKEN_BY}" ]; then \
            useradd -r -u "${APP_UID}" -g "${TARGET_GROUP}" -s /sbin/nologin -d /var/lib/warden "${APP_USER}"; \
        else \
            useradd -r -g "${TARGET_GROUP}" -s /sbin/nologin -d /var/lib/warden "${APP_USER}"; \
        fi; \
    fi; \
    mkdir -p /var/lib/warden && chown "${APP_USER}:${TARGET_GROUP}" /var/lib/warden

# Create directory structure with proper permissions
# /etc/warden       - Configuration files (read-only at runtime)
# /var/lib/warden   - Working directory and local backups
# /var/lib/node_exporter - Metrics output for Prometheus
# /tmp/warden       - Temporary files with proper cleanup
RUN set -eux; \
    EXISTING_GROUP="$(getent group "${APP_GID}" | cut -d: -f1 || true)"; \
    TARGET_GROUP="${EXISTING_GROUP:-${APP_GROUP}}"; \
    mkdir -p \
        /etc/warden \
        /var/lib/warden/backups \
        /var/lib/warden/wal \
        /var/lib/warden/logs \
        /var/lib/node_exporter \
        /tmp/warden; \
    chown -R "${APP_USER}:${TARGET_GROUP}" \
        /var/lib/warden \
        /var/lib/node_exporter \
        /tmp/warden; \
    chmod 755 /etc/warden; \
    chmod 750 /var/lib/warden /var/lib/warden/backups /var/lib/warden/wal /var/lib/warden/logs; \
    chmod 1777 /tmp/warden

# Copy default configuration files
# These serve as templates; mount your own configs at runtime
COPY --chown=root:${APP_GROUP} deploy/docker/config.yaml.example /etc/warden/config.yaml
COPY --chown=root:${APP_GROUP} deploy/docker/cluster.yaml.example /etc/warden/cluster.yaml
RUN chmod 640 /etc/warden/*.yaml

# Copy the warden binary from builder and verify it's executable
COPY --from=builder --chown=root:root /usr/src/warden/target/release/warden /usr/local/bin/warden
RUN chmod 755 /usr/local/bin/warden && \
    # Verify binary is functional and not corrupted
    /usr/local/bin/warden --version

# Security: Create a more restrictive umask for the application
RUN echo "umask 027" >> /etc/profile

# Set working directory
WORKDIR /var/lib/warden

# Switch to non-root user for security
USER ${APP_USER}

# Enhanced health check - verify warden binary and basic functionality
HEALTHCHECK --interval=30s --timeout=15s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/warden", "--version"] || exit 1

# Expose metrics port for Prometheus monitoring
EXPOSE ${WARDEN_METRICS_PORT}

# Volume mount points with explicit documentation
# Mount your configuration: -v /host/config:/etc/warden:ro
# Mount backup storage:     -v /host/backups:/var/lib/warden/backups
# Mount WAL storage:        -v /host/wal:/var/lib/warden/wal  
# Mount logs:               -v /host/logs:/var/lib/warden/logs
# Mount metrics output:     -v /host/node_exporter:/var/lib/node_exporter
VOLUME ["/etc/warden", "/var/lib/warden/backups", "/var/lib/warden/wal", "/var/lib/warden/logs", "/var/lib/node_exporter"]

# Signal handling for graceful shutdown
STOPSIGNAL SIGTERM

# Default command - run daemon in foreground with proper signal handling
# Override with: docker run warden postgresql snapshot-backup ...
CMD ["/usr/local/bin/warden", "run"]
