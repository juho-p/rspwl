# rspwl

Wayland compositor based on wlroots, written in Rust.

## Goal

A tiling wayland compositor that tries to imitate BSPWM model. Use the
excellent wlroots project to get good support for most useful wayland protocol
extensions.

It is not a plan to create safe wrapper around wlroots. The API is too wide for
me to consider that. So lets just use a lot of unsafe Rust instead.

## Current status

**Not ready for daily use**

### Works
- Composing windows (kinda, all the top levels are put into same coordinates)
- Effiecient(ish) rendering with damage tracking (mostly thanks to wlroots)
- xdg-shell with all the fancy popups and stuff

### In the future
- make it easy to build
- basic tiling
- background images (now there's no bg, and it is never redrawn. imagine the fun of that)
- floating windows with moving and resizing
- BSPWM style ipc (bspc -> rspc) with some basic configuration
- layer-shell
- advanced tiling
- advanced configuration
- other useful protocols
- xwayland support
