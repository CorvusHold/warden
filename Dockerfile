FROM ubuntu:24.04

ENV APP_USER=appuser \
    APP_DIR=/app

RUN groupadd -r ${APP_USER} && \
    useradd -r -g ${APP_USER} -s /bin/bash -d ${APP_DIR} ${APP_USER} && \
    mkdir -p ${APP_DIR} && \
    chown -R ${APP_USER}:${APP_USER} ${APP_DIR}

RUN apt update && \
    apt install -y --no-install-recommends \
    ca-certificates \
    postgresql-common \
    libssl3 && \
    apt update && \
    apt install -y postgresql-client-15 && \
    apt clean && \
    rm -rf /var/lib/apt/lists/*

# Switch to the app directory
WORKDIR ${APP_DIR}

# Copy the built binary from the release artifact
# This expects the binary name to match the one in your Cargo.toml
COPY --chown=${APP_USER}:${APP_USER} warden ${APP_DIR}/

# Set the binary as executable just to be safe
RUN chmod +x ${APP_DIR}/*

# Switch to non-root user
USER ${APP_USER}

CMD ["./warden"]