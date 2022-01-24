# rspwl

Wayland compositor based on wlroots, written in Rust. For build instructions, see the end of the README.

**Not ready for any practical use**

## Goal

A tiling wayland compositor that tries to imitate BSPWM model. Use the
excellent wlroots project to get good support for most useful wayland protocol
extensions.

It is not a plan to create safe wrapper around wlroots. The API is too wide for
me to consider that. So lets just use a lot of unsafe Rust instead.

## Current status

It's not ready yet.

### Works
- Composing windows (kinda, all the top levels are put into same coordinates)
- Effiecient(ish) rendering with damage tracking (mostly thanks to wlroots)
- xdg-shell with all the fancy popups and stuff
- Very basic tiling (but it's pretty bad)

### In the future
- better tiling
- background images (now there's no bg, and it is never redrawn. imagine the fun of that)
- floating windows with moving and resizing
- workspaces
- BSPWM style ipc (bspc -> rspc) with some basic configuration
- layer-shell
- advanced tiling
- advanced configuration
- other useful protocols
- xwayland support
- make it easier to build
- everything else

### Build instructions

- You'll need recent'ish stable rust toolchain
- You'll also need a C-compiler, pkg-config and probably quite a lot other build tools
- Dynamically linked libraries & their headers ("dev-packages")

Libraries needed: wayland-protocols, wayland-server, xkbcommon, pixman-1,
wlroots and libclang

wlroots must be at least 13.0, libclang is used for just the build step
(generate Rust bindings for the libraries listed there)

After everything is installed, just run `cargo run` to compile and run. By
default, if you are running under X or Wayland already, wlroots opens a window
that contains the compositor under your WM, which is preferred way to test this
at the moment.

See `test-builds/` directory for examples.
