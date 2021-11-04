use std::ffi::c_void;
use std::marker::PhantomPinned;
use std::pin::Pin;
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

// cool safe abstraction, but is is needed?
// TODO: remove this if it is still not used in the future
pub struct ListenerClosure {
    wl_listener: wl::wl_listener,
    f: Box<dyn Fn(*mut c_void) -> ()>,
    _pin: PhantomPinned,
}

pub type ListenerFn = Pin<Box<ListenerClosure>>;

impl ListenerClosure {
    pub fn listen_signal(
        signal: &mut wl::wl_signal,
        f: Box<dyn Fn(*mut c_void) -> ()>,
    ) -> ListenerFn {
        let mut listener = Box::pin(ListenerClosure {
            wl_listener: new_wl_listener(Some(listener_notify)),
            f,
            _pin: PhantomPinned::default(),
        });

        unsafe {
            let mut_ref: Pin<&mut Self> = Pin::as_mut(&mut listener);
            let listener = Pin::get_unchecked_mut(mut_ref);
            signal_add(signal, &mut listener.wl_listener);
        }

        listener
    }
}
unsafe extern "C" fn listener_notify(wl_listener: *mut wl::wl_listener, data: *mut c_void) {
    let listener = container_of!(ListenerClosure, wl_listener, wl_listener);
    ((*listener).f)(data);
}
