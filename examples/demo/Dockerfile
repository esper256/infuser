FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
RUN cargo build --locked --release --bin infuser-demo

FROM debian:bookworm-slim
RUN mkdir /data && chown 1000:1000 /data
COPY --from=build /src/target/release/infuser-demo /usr/local/bin/infuser-demo
USER 1000:1000
EXPOSE 7000
HEALTHCHECK --interval=1s --timeout=3s --start-period=2s --retries=3 CMD ["/usr/local/bin/infuser-demo", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/infuser-demo"]
