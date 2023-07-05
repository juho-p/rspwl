mod types;
mod tree;
mod window_manager;
mod wl_util;
mod wlroots_compositor;

fn main() {
    wlroots_compositor::runner::run_server();
}
