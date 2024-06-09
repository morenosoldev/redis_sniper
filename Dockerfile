# Use the official Rust image as the base
FROM rust:latest

# Set the working directory
WORKDIR /usr/src/myapp

# Copy the Cargo.toml and Cargo.lock files
COPY Cargo.toml Cargo.lock ./

# Copy the source code
COPY src ./src

# Build the Rust application
RUN cargo build --release

# Set the entrypoint command to run the compiled binary
CMD ["./target/release/redis_main_project"]
