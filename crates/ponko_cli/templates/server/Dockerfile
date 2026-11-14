# syntax=docker/dockerfile:1
#
# The server's container image: `docker build -t {{project-name}} .`, then
#   docker run --rm -p 7777:7777/udp {{project-name}}
# Stop it with `docker stop`: the server gets SIGTERM, says goodbye to its
# clients and exits (https://ponko.dev/guide/servers.html).
#
# Two stages: `ponko package` builds the server in a Rust image (release
# build, assets.pack, licenses, THIRD-PARTY-NOTICES.txt, then a smoke run),
# and only that package goes into a small runtime image. To package
# elsewhere (on a Linux CI machine, say), put the package folder at
# `package/` and build with `--build-arg PACKAGE_FROM=prebuilt`.

# Where the package comes from: `build` (here) or `prebuilt` (./package).
ARG PACKAGE_FROM=build
ARG RUST_VERSION=1.95

FROM rust:${RUST_VERSION}-slim-bookworm AS build
# The `ponko` command, without its texture tools (quicker to install).
ARG PONKO_CLI=ponko_cli
# PONKO_CLI may hold several words (`--git <url> ponko_cli`): split on purpose.
# hadolint ignore=SC2086
RUN cargo install --locked --no-default-features ${PONKO_CLI}
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    ponko package --format folder --out /out \
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
ENV SERVER_ADDR=0.0.0.0:7777
EXPOSE 7777/udp
# What `docker stop` sends; the server's shutdown grace is 3 s of its 10.
STOPSIGNAL SIGTERM
USER nonroot
ENTRYPOINT ["/app/{{project-name}}"]
