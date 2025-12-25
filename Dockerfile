# Warden PostgreSQL Data Protection Agent
# Multi-stage build for minimal production image
#
# Build: docker build -t warden:latest .
# Run:   docker run -v /path/to/config:/etc/warden warden:latest

# =============================================================================
# Stage 1: Base image with PostgreSQL client tools
# =============================================================================
FROM ubuntu:24.04 AS base

# Install PostgreSQL client tools and minimal dependencies
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        postgresql-common \
        libssl3 \
        curl && \
    yes | sh /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh && \
    apt-get update && \
    apt-get install -y --no-install-recommends postgresql-client-17 && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# =============================================================================
# Stage 2: Production image
# =============================================================================
FROM base AS production

# Labels for container metadata
LABEL org.opencontainers.image.title="Warden" \
      org.opencontainers.image.description="PostgreSQL Data Protection Agent" \
      org.opencontainers.image.vendor="Corvus" \
      org.opencontainers.image.source="https://github.com/corvushold/warden"

# Environment configuration
ENV APP_USER=warden \
    APP_GROUP=warden \
    APP_UID=1000 \
    APP_GID=1000 \
    WARDEN_CONFIG=/etc/warden/config.yaml \
    WARDEN_CLUSTER_CONFIG=/etc/warden/cluster.yaml \
    WARDEN_LOG_LEVEL=info

# Create non-root user with specific UID/GID for consistent permissions
RUN groupadd -r -g ${APP_GID} ${APP_GROUP} && \
    useradd -r -u ${APP_UID} -g ${APP_GROUP} -s /sbin/nologin -d /var/lib/warden ${APP_USER}

# Create directory structure with proper permissions
# /etc/warden       - Configuration files (read-only at runtime)
# /var/lib/warden   - Working directory and local backups
# /var/lib/node_exporter - Metrics output for Prometheus
RUN mkdir -p \
        /etc/warden \
        /var/lib/warden/backups \
        /var/lib/warden/wal \
        /var/lib/node_exporter && \
    chown -R ${APP_USER}:${APP_GROUP} \
        /var/lib/warden \
        /var/lib/node_exporter && \
    chmod 755 /etc/warden && \
    chmod 750 /var/lib/warden /var/lib/warden/backups /var/lib/warden/wal

# Copy default configuration files
# These serve as templates; mount your own configs at runtime
COPY --chown=root:${APP_GROUP} deploy/docker/config.yaml.example /etc/warden/config.yaml
COPY --chown=root:${APP_GROUP} deploy/docker/cluster.yaml.example /etc/warden/cluster.yaml
RUN chmod 640 /etc/warden/*.yaml

# Copy the warden binary
COPY --chown=root:root warden /usr/local/bin/warden
RUN chmod 755 /usr/local/bin/warden

# Set working directory
WORKDIR /var/lib/warden

# Switch to non-root user
USER ${APP_USER}

# Health check - verify warden binary is functional
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/warden", "--version"]

# Expose metrics port (if running HTTP metrics server in future)
# EXPOSE 9090

# Volume mount points
# Mount your configuration: -v /host/config:/etc/warden:ro
# Mount backup storage:     -v /host/backups:/var/lib/warden/backups
# Mount metrics output:     -v /host/node_exporter:/var/lib/node_exporter
VOLUME ["/etc/warden", "/var/lib/warden/backups", "/var/lib/node_exporter"]

# Default command - run daemon in foreground
# Override with: docker run warden postgresql snapshot-backup ...
CMD ["/usr/local/bin/warden", "run"]
