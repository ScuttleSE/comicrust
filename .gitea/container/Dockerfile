# The CI/release build image for the comicrust workflows.
#
# Build it ON the runner host (the machine running act_runner) so the
# docker daemon has it locally and no registry login is needed:
#
#   docker build -t comicrust-ci:latest - < .gitea/container/Dockerfile
#
# act_runner uses a locally present image as-is (it only pulls when the
# image is missing). Alternatively push it to the Gitea container
# registry and change the workflow image references to
# git.hemmalab.se/<owner>/comicrust-ci:latest.
#
# Rebuild after Debian point releases or to pick up a newer Rust
# stable (rustup installs the current stable at build time).

FROM debian:trixie

ENV DEBIAN_FRONTEND=noninteractive

# libgtk-4-dev pulls in glib/gdk/cairo/pango dev libraries for cr-ui.
# p7zip-full and djvulibre-bin enable the gated cr-io format tests
# (CR_FORMAT_TESTS=1 / tools on PATH). PDF tests stay ungated-off:
# they need a libpdfium.so we do not ship.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        git \
        curl \
        ca-certificates \
        jq \
        xz-utils \
        libgtk-4-dev \
        p7zip-full \
        djvulibre-bin \
    && rm -rf /var/lib/apt/lists/*

# Latest stable Rust. Minimal profile (no docs), clippy and rustfmt
# included because the CI gate runs them.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:${PATH}
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal --component clippy,rustfmt \
    && rustc --version \
    && cargo --version
