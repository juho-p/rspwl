mod wl_util;
mod wlroots_compositor;

#[macro_use]
extern crate log;

fn main() {
    // TODO rust logger overlaps with wlr logger, FIX IT
    pretty_env_logger::formatted_builder()
        .parse_filters("info")
        .init();

    println!("Hello, world!");

    wlroots_compositor::runner::run_server();
}
