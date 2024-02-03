# Important: This file is provided for demonstration purposes and may NOT be suitable for production use.
# The maintainers of electrs are not deeply familiar with Docker, so you should DYOR.
# If you are not familiar with Docker either it's probably be safer to NOT use it.

FROM debian:bookworm-slim as base
RUN apt-get update -qqy
RUN apt-get install -qqy librocksdb-dev curl

### Electrum Rust Server ###
RUN apt-get install -qqy cargo clang cmake
WORKDIR /build/electrs
COPY . .
ENV ROCKSDB_INCLUDE_DIR=/usr/include
ENV ROCKSDB_LIB_DIR=/usr/lib
RUN cargo build
CMD /build/electrs/target/debug/metashrew
