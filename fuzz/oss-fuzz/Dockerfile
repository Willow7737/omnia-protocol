# Omnia Protocol — OSS-Fuzz Integration
#
# This Dockerfile builds the fuzz targets for submission to Google OSS-Fuzz.
# Fuzz targets are compiled with AddressSanitizer (ASan) and coverage
# instrumentation for continuous fuzzing on the OSS-Fuzz infrastructure.
#
# To use locally:
#   docker build -t omnia-fuzz -f fuzz/oss-fuzz/Dockerfile .
#   docker run --rm omnia-fuzz /out/fuzz_event_deserialization
#
# To submit to OSS-Fuzz:
#   1. Fork https://github.com/google/oss-fuzz
#   2. Add omnia-protocol to projects/
#   3. Use this Dockerfile as the project's Dockerfile

FROM rust:1.85-bookworm AS builder

RUN apt-get update && apt-get install -y \
    clang libclang-dev llvm-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-fuzz

WORKDIR /src
COPY . .

# Build all fuzz targets with AddressSanitizer
RUN cargo fuzz build -O --debug-assertions

FROM gcr.io/oss-fuzz-base/base-runner
COPY --from=builder /src/fuzz/target/x86_64-unknown-linux-gnu/release/ /out/
