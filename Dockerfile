FROM rust:1.92.0-trixie AS builder

# Install cargo-binstall, which makes it easier to install other
# cargo extensions like cargo-leptos
RUN wget https://github.com/cargo-bins/cargo-binstall/releases/latest/download/cargo-binstall-x86_64-unknown-linux-musl.tgz
RUN tar -xvf cargo-binstall-x86_64-unknown-linux-musl.tgz
RUN cp cargo-binstall /usr/local/cargo/bin

# Install required tools
RUN apt-get update -y \
  && apt-get install -y --no-install-recommends clang

# Install cargo-leptos
RUN cargo binstall cargo-leptos -y

WORKDIR /src
RUN git clone --depth 1 --single-branch --branch v0.9.2 https://github.com/HarryDeKroon/ol-timesheet.git .

# Add the WASM target
RUN rustup target add wasm32-unknown-unknown

# Build the app
RUN cargo leptos build --release -vv

FROM debian:trixie-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
  && apt-get install -y --no-install-recommends openssl ca-certificates \
  && apt-get autoremove -y \
  && apt-get clean -y \
  && rm -rf /var/lib/apt/lists/*

# -- NB: update binary name from "leptos_start" to match your app name in Cargo.toml --
# Copy the server binary to the /app directory
COPY --from=builder /src/target/release/timesheet /app/

# /target/site contains our JS/WASM/CSS, etc.
COPY --from=builder /src/target/site /app/site

# Copy Cargo.toml if it’s needed at runtime
COPY --from=builder /src/Cargo.toml /app/

ENV RUST_LOG=info
ENV XDG_CONFIG_HOME=/root/.config/Timesheet
ENV LEPTOS_OUTPUT_NAME=timesheet
ENV LEPTOS_SITE_ROOT=site
ENV LEPTOS_SITE_PKG_DIR=pkg
ENV LEPTOS_SITE_ADDR=0.0.0.0:8081

EXPOSE 8081

ENTRYPOINT ["/app/timesheet"]
