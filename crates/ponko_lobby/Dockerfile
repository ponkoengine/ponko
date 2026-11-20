# syntax=docker/dockerfile:1
#
# The lobby server's container image, the dedicated-server recipe of
# `ponko new --server` (the guide's "Dedicated servers" chapter) for the
# `ponko-lobby` program. Build it from the root of the engine's repository:
#   docker build -f crates/ponko_lobby/Dockerfile -t ponko-lobby .
#   docker run --rm -p 7700:7700/udp -p 7701:7701/tcp ponko-lobby
# Stop it with `docker stop`: on SIGTERM members hear that the server stops,
# then it exits (within 2 s).
#
# Two stages: `ponko package` builds the server in a Rust image (release
# build, licenses, THIRD-PARTY-NOTICES.txt, then a smoke run), and only that
# package goes into a small runtime image. To package elsewhere, put the
# package folder at `package/` (in the repository root) and build with
# `--build-arg PACKAGE_FROM=prebuilt`.

# Where the package comes from: `build` (here) or `prebuilt` (./package).
ARG PACKAGE_FROM=build
ARG RUST_VERSION=1.95

FROM rust:${RUST_VERSION}-slim-bookworm AS build
WORKDIR /src
COPY . .
# The `ponko` command from this checkout, without its texture tools, then
# the package.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo install --locked --no-default-features --path crates/ponko_cli \
    && ponko package -p ponko_lobby --format folder --out /out \
    && mv /out/*/ /package

FROM scratch AS prebuilt
COPY package/ /package/

# One of the two stages above, by name.
# hadolint ignore=DL3006
FROM ${PACKAGE_FROM} AS package

# glibc and libgcc, no shell or package manager, a non-root user.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=package /package /app
WORKDIR /app
# UDP for desktop players, WebSocket (plain ws://; put a TLS proxy in front
# for wss://) for browsers. `ponko-lobby --help` lists every setting.
ENV LOBBY_UDP=0.0.0.0:7700 \
    LOBBY_WEB=0.0.0.0:7701
EXPOSE 7700/udp 7701/tcp
STOPSIGNAL SIGTERM
USER nonroot
ENTRYPOINT ["/app/ponko-lobby"]
