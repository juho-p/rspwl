extern crate bindgen;

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    pkg_config_check(&["--exact-version=0.13.0", "wlroots", "--print-errors"]);

    let libs = [
        "wayland-protocols",
        "wayland-server",
        "xkbcommon",
        "pixman-1",
        "wlroots",
    ];

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    std::fs::create_dir_all(&out_path).unwrap();

    let protocol_dir = pkg_config_args(&["wayland-protocols"], "--variable=pkgdatadir");
    let xdg_protocol = format!("{}/stable/xdg-shell/xdg-shell.xml", protocol_dir);

    let ok = Command::new("wayland-scanner")
        .arg("server-header")
        .arg(&xdg_protocol)
        .arg(&out_path.join("xdg-shell-protocol.h"))
        .status()
        .unwrap()
        .success();
    if !ok {
        panic!("wayland-scanner failed");
    }

    let c_code = out_path.join("xdg-shell-protocol.c");
    let ok = Command::new("wayland-scanner")
        .arg("private-code")
        .arg(&xdg_protocol)
        .arg(&c_code)
        .status()
        .unwrap()
        .success();
    if !ok {
        panic!("wayland-scanner failed");
    }

    cc::Build::new().file(&c_code).compile("xdg-shell-protocol");

    let builder = bindgen::builder()
        .header("wrapper.h")
        .parse_callbacks(Box::new(BindgenWorkaround {}))
        .allowlist_function("wl.*")
        .allowlist_function("pixman.*")
        .allowlist_function("xkb_.*")

        // bit hairy, but deal with it
        .allowlist_function("clock_gettime")
        .allowlist_var("CLOCK_MONOTONIC")
        .allowlist_var("XKB_.*")

        .allowlist_type("wl.*")
        .allowlist_type("pixman.*")
        .allowlist_type("xkb_.*")
        .clang_arg(format!("-I{}", out_path.display()))
        .clang_args(
            pkg_config_args(&libs, "--cflags")
                .split(' ')
                .map(|x| x.to_string()),
        );

    let bindings = builder.generate().unwrap();

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .unwrap();

    println!("cargo:rerun-if-changed=wrapper.h");

    for pkg_arg in pkg_config_args(&libs, "--libs").split(' ') {
        if pkg_arg.starts_with("-l") {
            println!("cargo:rustc-link-lib=dylib={}", &pkg_arg[2..]);
        } else if pkg_arg.starts_with("-L") {
            println!("cargo:rustc-link-search=native={}", &pkg_arg[2..]);
        } else {
            println!("cargo:rustc-link-arg={}", pkg_arg);
        }
    }
}

#[derive(Debug)]
struct BindgenWorkaround;
impl bindgen::callbacks::ParseCallbacks for BindgenWorkaround {
    fn will_parse_macro(&self, name: &str) -> bindgen::callbacks::MacroParsingBehavior {
        let ignore = [
            "FP_NAN",
            "FP_INFINITE",
            "FP_ZERO",
            "FP_SUBNORMAL",
            "FP_NORMAL",
        ]
        .iter()
        .any(|x| *x == name);
        if ignore {
            bindgen::callbacks::MacroParsingBehavior::Ignore
        } else {
            bindgen::callbacks::MacroParsingBehavior::Default
        }
    }
}

fn pkg_config_check(args: &[&str]) {
    let mut cmd = Command::new("pkg-config");
    cmd.args(args);
    let output = cmd.output().unwrap();
    if !output.status.success() {
        panic!(
            "pkg-config failed: {:?}",
            std::str::from_utf8(&output.stderr).unwrap()
        );
    }
}

fn pkg_config_args(libs: &[&str], arg: &str) -> String {
    let mut cmd = Command::new("pkg-config");
    cmd.arg(arg);
    cmd.args(libs);
    let output = cmd.output().unwrap();
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .to_string()
}
