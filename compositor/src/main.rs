mod wl_util;
mod wlroots_compositor;

fn main() {
    println!("Hello, world!");

    wlroots_compositor::run_server();
}
