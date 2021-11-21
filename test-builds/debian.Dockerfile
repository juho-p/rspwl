# This actually uses older libwlroots which is much easier to get built on Debian stable

FROM debian:bullseye

RUN apt-get update && \
    apt-get install -y build-essential libegl-dev libgles-dev libdrm-dev libgbm-dev \
    libinput-dev libxkbcommon-dev libudev-dev libpixman-1-dev libxml2-dev \
    libxcb-dri3-dev libxcb-present-dev python3 python3-pip libsystemd-dev \
    ninja-build curl libclang-11-dev git

RUN python3 -m pip install meson

WORKDIR /tmp

RUN git clone https://git.sr.ht/~kennylevinsen/seatd && \
    cd seatd && \
    meson build/ && \
    cd build && \
    ninja && \
    ninja install && \
    rm -rf seatd

RUN git clone https://gitlab.freedesktop.org/wayland/wayland && \
    cd wayland && \
    meson -Ddocumentation=false build/ && \
    cd build && \
    ninja && \
    ninja install && \
    rm -rf wayland

RUN git clone https://gitlab.freedesktop.org/wayland/wayland-protocols.git && \
    cd wayland-protocols && \
    meson build/ && \
    cd build && ninja && ninja install && \
    rm -rf wayland-protocols

# And now for the *old* wlroots
RUN git clone https://gitlab.freedesktop.org/wlroots/wlroots.git && \
    cd wlroots && \
    git checkout 0.13.0 && \
    meson build/ && \
    cd build && \
    ninja && \
    ninja install && \
    rm -rf wlroots

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

COPY test-builds/deps-dummy/ .
RUN export PATH=$HOME/.cargo/bin:$PATH && cargo build && rm -rf target

RUN mkdir -p /build/rspwl && ldconfig

WORKDIR /build/rspwl

COPY wl-sys wl-sys
COPY compositor compositor
COPY Cargo.toml .

RUN export PATH=$HOME/.cargo/bin:$PATH && cargo build
