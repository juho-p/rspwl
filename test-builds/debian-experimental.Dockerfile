FROM debian:experimental

RUN apt-get update && \
    apt-get install -t experimental -y build-essential libgles-dev \
    libwayland-dev libwayland-bin libwayland-server0 libxkbcommon-dev libpixman-1-dev \
    libclang-dev libwlroots-dev \
    ninja-build meson curl

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

WORKDIR /tmp
COPY test-builds/deps-dummy/ .
RUN export PATH=$HOME/.cargo/bin:$PATH && cargo fetch

RUN mkdir -p /build/rspwl
WORKDIR /build/rspwl

RUN apt-get install -y 

COPY wl-sys wl-sys
COPY compositor compositor
COPY Cargo.toml .

RUN export PATH=$HOME/.cargo/bin:$PATH && cargo build
