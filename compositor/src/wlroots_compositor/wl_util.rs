use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;

use wl_sys as wl;

macro_rules! cstring {
    ($string_literal:expr) => {
        concat!($string_literal, "\0").as_ptr() as *const std::os::raw::c_char
    };
}
pub(crate) use cstring;

pub fn new_wl_listener(
    notify: Option<unsafe extern "C" fn(*mut wl::wl_listener, *mut c_void)>,
) -> wl::wl_listener {
    wl::wl_listener {
        link: wl::wl_list {
            // must setup these later with `signal_add`
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        },
        notify,
    }
}

pub fn signal_add(signal: &mut wl::wl_signal, listener: &mut wl::wl_listener) {
    unsafe {
        // it's the same as wayland-server-core.h:wl_signal_add
        wl::wl_list_insert(signal.listener_list.prev, &mut listener.link);
    }
}

// Macro version of C wl_container_of
macro_rules! container_of {
    ($parent:path, $field:tt, $value:ident) => {{
        use memoffset::offset_of;
        let offset = offset_of!($parent, $field);
        ($value as usize - offset) as *mut $parent
    }};
}
pub(crate) use container_of;

pub fn xdg_surface_for_each_surface<F: Fn(&mut wl::wlr_surface, i32, i32) -> ()>(
    xdg_surface: *mut wl::wlr_xdg_surface,
    f: F,
) {
    struct IterData<F: Fn(&mut wl::wlr_surface, i32, i32) -> ()> {
        f: F,
    }
    unsafe extern "C" fn do_damage<F: Fn(&mut wl::wlr_surface, i32, i32) -> ()>(
        surface: *mut wl::wlr_surface,
        sx: c_int,
        sy: c_int,
        data: *mut c_void,
    ) {
        let data = &*(data as *const IterData<F>);
        (data.f)(&mut *surface, sx, sy);
    }
    let mut data = IterData { f };
    unsafe {
        wl::wlr_xdg_surface_for_each_surface(
            xdg_surface,
            Some(do_damage::<F>),
            &mut data as *mut IterData<F> as *mut c_void,
        );
    }
}

pub struct ListenerWrapper(pub wl::wl_listener);
impl Drop for ListenerWrapper {
    fn drop(&mut self) {
        if self.0.link.prev.is_null() {
            panic!("BUG: Listener is not setup!");
        }
        unsafe {
            wl::wl_list_remove(&mut self.0.link);
        }
    }
}

pub struct Point {
    pub x: f64,
    pub y: f64,
}
