# Build stage
FROM rust:1.93-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/gaise

# Copy the entire workspace
COPY . .

# Build the gaise-api project
RUN cargo build --release -p gaise-api

# Run stage
FROM debian:bookworm-slim

# Install runtime dependencies
# RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

# Copy the binary from the builder stage
COPY --from=builder /usr/src/gaise/target/release/gaise-api .

# Expose the default port
EXPOSE 3000

# Set environment variables
ENV GAISE_PORT=3000

# Run the binary
CMD ["./gaise-api"]
