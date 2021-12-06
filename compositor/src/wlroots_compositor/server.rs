use std::ffi::c_void;
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr;

use wl_sys as wl;

use crate::window_manager::WindowManager;

use super::wl_util::*;

pub type NodeId = u32;
pub type OutputId = u8;
pub type KeyboardId = u8;

static mut SERVER_GLOBAL: *mut Server = ptr::null_mut();

pub unsafe fn server_ptr() -> *mut Server {
    if SERVER_GLOBAL.is_null() {
        panic!("BUG! Uninitialized.");
    } else {
        SERVER_GLOBAL
    }
}
pub unsafe fn set_server(server: *mut Server) {
    SERVER_GLOBAL = server;
}

pub struct Server {
    pub wl_display: *mut wl::wl_display,
    pub backend: *mut wl::wlr_backend,
    pub renderer: *mut wl::wlr_renderer,

    pub xdg_shell: *mut wl::wlr_xdg_shell,

    pub cursor: *mut wl::wlr_cursor,
    pub cursor_mgr: *mut wl::wlr_xcursor_manager,
    pub cursor_motion: Listener<wl::wlr_event_pointer_motion, ()>,
    pub cursor_motion_absolute: Listener<wl::wlr_event_pointer_motion_absolute, ()>,
    pub cursor_button: Listener<wl::wlr_event_pointer_button, ()>,
    pub cursor_axis: Listener<wl::wlr_event_pointer_axis, ()>,
    pub cursor_frame: Listener<(), ()>,

    pub seat: *mut wl::wlr_seat,
    pub new_input: Listener<wl::wlr_input_device, ()>,
    pub request_cursor: Listener<wl::wlr_seat_pointer_request_set_cursor_event, ()>,
    pub request_set_selection: Listener<wl::wlr_seat_request_set_selection_event, ()>,
    pub keyboards: Vec<Pin<Box<Keyboard>>>,

    pub output_layout: *mut wl::wlr_output_layout,
    pub outputs: Vec<Pin<Box<Output>>>,

    pub new_xdg_surface: Listener<wl::wlr_xdg_surface, ()>,
    pub new_output: Listener<wl::wlr_output, ()>,

    pub wm: WindowManager,
}

impl Server {
    pub fn new_output_id(&self) -> OutputId {
        (1..255)
            .find(|id| self.outputs.iter().all(|output| output.id != *id))
            .expect("Too many outputs!")
    }
}

pub struct Output {
    pub id: OutputId,

    pub wlr_output: *mut wl::wlr_output,
    pub damage: *mut wl::wlr_output_damage,
    pub damage_frame: wl::wl_listener,
    pub destroy: Listener<(), OutputId>,

    pub _pin: PhantomPinned,
}

impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.damage_frame.link);
        }
    }
}

pub struct Keyboard {
    pub device: *mut wl::wlr_input_device,
    pub modifiers: Listener<(), u8>,
    pub key: Listener<wl::wlr_event_keyboard_key, u8>,
    pub id: KeyboardId,
}

pub struct Listener<Data, Ctx: Copy> {
    listener: ListenerWrapper,
    ctx: Ctx,
    f: fn(&mut Server, &mut Data, Ctx),
}

impl<Data, Ctx: Copy> Listener<Data, Ctx> {
    pub fn new(f: fn(&mut Server, &mut Data, Ctx) -> (), ctx: Ctx) -> Self {
        Listener {
            listener: ListenerWrapper(new_wl_listener(Some(server_notify::<Data, Ctx>))),
            ctx,
            f,
        }
    }
}
unsafe extern "C" fn server_notify<Data, Ctx: Copy>(
    wl_listener: *mut wl::wl_listener,
    data: *mut c_void,
) {
    let p = container_of!(Listener<Data, Ctx>, listener, wl_listener);
    ((*p).f)(&mut *server_ptr(), &mut *(data as *mut Data), (*p).ctx);
}

// After this call, do NOT move the listener (yeah, yeah, should pin. maybe later)
pub unsafe fn listen_server_signal<Data, Ctx: Copy>(
    signal: &mut wl::wl_signal,
    listener: &mut Listener<Data, Ctx>,
) {
    signal_add(signal, &mut listener.listener.0);
}

pub enum ShellView {
    Empty,
    Xdg(XdgView),
    // later: layer-shell, xwayland ?
}

impl ShellView {
    fn configure_size(&self, w: u32, h: u32) {
        match self {
            ShellView::Empty => (),
            ShellView::Xdg(xdgview) => {
                unsafe {
                    wl::wlr_xdg_toplevel_set_size(xdgview.xdgsurface.xdg_surface, w, h);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct View {
    pub id: NodeId,

    pub shell_surface: ShellView,

    pub x: i32,
    pub y: i32,

    children: Vec<Pin<Box<ViewChild>>>,

    _pin: PhantomPinned,
}

impl View {
    pub unsafe fn from_xdg_toplevel_surface(
        id: NodeId,
        xdg_surface: *mut wl::wlr_xdg_surface,
    ) -> Pin<Box<Self>> {
        let mut view = Box::pin(View {
            id,
            shell_surface: ShellView::Empty,
            x: 70,
            y: 5,
            children: Vec::new(),

            _pin: PhantomPinned,
        });

        let x = view.as_mut().get_unchecked_mut();
        x.shell_surface = ShellView::Xdg(XdgView::new(x, xdg_surface));
        x.configure_listeners();

        view
    }

    unsafe fn configure_listeners(&mut self) {
        match &mut self.shell_surface {
            ShellView::Empty => (),
            ShellView::Xdg(v) => v.configure_listeners(),
        }
    }

    pub fn configure_rect(self: Pin<&mut Self>, rect: &Rect) {
        self.shell_surface.configure_size(rect.w.round() as u32, rect.h.round() as u32);
        unsafe {
            let borrowed = self.get_unchecked_mut();
            borrowed.x = rect.x.round() as i32;
            borrowed.y = rect.y.round() as i32;
        }
    }
}

pub struct XdgView {
    pub xdgsurface: XdgSurface,
    toplevel: *mut wl::wlr_xdg_toplevel,
    request_move: wl::wl_listener,
    request_resize: wl::wl_listener,

    _pin: PhantomPinned,
}

impl XdgView {
    unsafe fn new(parent: *mut View, xdg_surface: *mut wl::wlr_xdg_surface) -> Self {
        let toplevel = &mut *(*xdg_surface).__bindgen_anon_1.toplevel;
        let surface = (*xdg_surface).surface;
        XdgView {
            xdgsurface: XdgSurface::new(parent, xdg_surface, surface, SurfaceBehavior::Toplevel),
            toplevel,

            request_move: new_wl_listener(Some(xdg_view_request_move)),
            request_resize: new_wl_listener(Some(xdg_view_request_resize)),

            _pin: PhantomPinned,
        }
    }

    // Can't be moved after this is called. Unsafe.
    unsafe fn configure_listeners(&mut self) {
        self.xdgsurface.configure_listeners();

        let x = &mut *self.toplevel;
        signal_add(&mut x.events.request_move, &mut self.request_move);
        signal_add(&mut x.events.request_resize, &mut self.request_resize);
    }
}
unsafe extern "C" fn xdg_view_request_move(_listener: *mut wl::wl_listener, _: *mut c_void) {
    println!("requested move but we don't care");
}
unsafe extern "C" fn xdg_view_request_resize(_listener: *mut wl::wl_listener, _: *mut c_void) {
    println!("requested resize but we don't care");
}

pub struct XdgSurface {
    pub surface: Surface,

    mapped: bool,

    pub xdg_surface: *mut wl::wlr_xdg_surface,
    map: wl::wl_listener,
    unmap: wl::wl_listener,
    new_popup: wl::wl_listener,
}

impl XdgSurface {
    fn new(
        parent: *mut View,
        xdg_surface: *mut wl::wlr_xdg_surface,
        wl_surface: *mut wl::wlr_surface,
        kind: SurfaceBehavior,
    ) -> Self {
        XdgSurface {
            surface: Surface::new(parent, wl_surface, kind),
            mapped: false,
            xdg_surface,
            map: new_wl_listener(Some(xdg_surface_map)),
            unmap: new_wl_listener(Some(xdg_surface_unmap)),
            new_popup: new_wl_listener(Some(xdg_surface_new_popup)),
        }
    }

    // Can't be moved after this is called. Unsafe.
    unsafe fn configure_listeners(&mut self) {
        self.surface.configure_listeners();
        let x = &mut *self.xdg_surface;
        signal_add(&mut x.events.map, &mut self.map);
        signal_add(&mut x.events.unmap, &mut self.unmap);
        signal_add(&mut x.events.new_popup, &mut self.new_popup);
    }
}

impl Drop for XdgSurface {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.map.link);
            wl::wl_list_remove(&mut self.unmap.link);
            wl::wl_list_remove(&mut self.new_popup.link);
        }
    }
}

unsafe extern "C" fn xdg_surface_map(listener: *mut wl::wl_listener, _: *mut c_void) {
    let it = &mut *container_of!(XdgSurface, map, listener);
    let view = &mut *it.surface.view;

    it.mapped = true;

    info!("Mapped {}", view.id);

    damage_view(&*server_ptr(), view, true);
}
unsafe extern "C" fn xdg_surface_unmap(listener: *mut wl::wl_listener, _: *mut c_void) {
    let it = &mut *container_of!(XdgSurface, unmap, listener);
    let view = &mut *it.surface.view;

    info!("Unmapped {}", view.id);

    damage_view(&*server_ptr(), view, true);
}
unsafe extern "C" fn xdg_surface_new_popup(listener: *mut wl::wl_listener, data: *mut c_void) {
    let it = &mut *container_of!(XdgSurface, new_popup, listener);
    let view = &mut *it.surface.view;

    info!("New popup");
    let popup = data as *mut wl::wlr_xdg_popup;
    let popup = &mut *popup;
    let xdg_surface = &mut *popup.base;
    let surface = xdg_surface.surface;

    view.children.push(ViewChild::new_popup(XdgSurface::new(
        it.surface.view,
        xdg_surface,
        surface,
        SurfaceBehavior::Child,
    )));
}

pub enum SurfaceBehavior {
    Toplevel,
    Child,
}
pub struct Surface {
    view: *mut View,
    pub surface: *mut wl::wlr_surface,
    commit: wl::wl_listener,
    destroy: wl::wl_listener,
    new_subsurface: wl::wl_listener,
    destroy_behaviour: SurfaceBehavior,
}

impl Surface {
    fn new(view: *mut View, surface: *mut wl::wlr_surface, behaviour: SurfaceBehavior) -> Self {
        Surface {
            view,
            surface,
            commit: new_wl_listener(Some(surface_commit)),
            destroy: new_wl_listener(Some(surface_destroy)),
            new_subsurface: new_wl_listener(Some(surface_new_subsurface)),
            destroy_behaviour: behaviour,
        }
    }

    unsafe fn configure_listeners(&mut self) {
        let x = &mut *self.surface;
        signal_add(&mut x.events.commit, &mut self.commit);
        signal_add(&mut x.events.destroy, &mut self.destroy);
        signal_add(&mut x.events.new_subsurface, &mut self.new_subsurface);
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.commit.link);
            wl::wl_list_remove(&mut self.destroy.link);
            wl::wl_list_remove(&mut self.new_subsurface.link);
        }
    }
}

unsafe extern "C" fn surface_commit(listener: *mut wl::wl_listener, _: *mut c_void) {
    let it = &mut *container_of!(Surface, commit, listener);
    let view = &mut *it.view;
    info!("commit");
    damage_view(&*server_ptr(), view, false);
}
unsafe extern "C" fn surface_destroy(listener: *mut wl::wl_listener, _: *mut c_void) {
    let it = &mut *container_of!(Surface, destroy, listener);
    let view = &mut *it.view;

    match it.destroy_behaviour {
        SurfaceBehavior::Toplevel => {
            let server = &mut *server_ptr();
            info!("Top level surface destroyed");
            server.wm.remove_node(view.id);
        }
        SurfaceBehavior::Child => {
            let wlr_surface = it.surface;

            if let Some(pos) = view.children.iter().position(|x| x.surface == wlr_surface) {
                std::mem::drop(it); // for clarity, the ref is invalid after next line
                view.children.swap_remove(pos);
            }
        }
    }
}
unsafe extern "C" fn surface_new_subsurface(listener: *mut wl::wl_listener, data: *mut c_void) {
    let it = &mut *container_of!(Surface, new_subsurface, listener);
    let view = &mut *it.view;

    info!("New subsurface");

    let subsurface = data as *mut wl::wlr_subsurface;
    let surface = (*subsurface).surface;

    view.children.push(ViewChild::new_subsurface(Surface::new(
        it.view,
        surface,
        SurfaceBehavior::Child,
    )));
}

enum PopupOrSubsurface {
    Popup(XdgSurface),
    Subsurface(Surface),
}
struct ViewChild {
    child: PopupOrSubsurface,
    surface: *mut wl::wlr_surface,

    _pin: PhantomPinned,
}

impl ViewChild {
    fn new_subsurface(surface: Surface) -> Pin<Box<Self>> {
        let wl_surface = surface.surface;
        ViewChild {
            child: PopupOrSubsurface::Subsurface(surface),
            surface: wl_surface,
            _pin: PhantomPinned,
        }
        .configure()
    }
    fn new_popup(xdgsurface: XdgSurface) -> Pin<Box<Self>> {
        let surface = xdgsurface.surface.surface;
        ViewChild {
            child: PopupOrSubsurface::Popup(xdgsurface),
            surface,
            _pin: PhantomPinned,
        }
        .configure()
    }
    fn configure(self) -> Pin<Box<Self>> {
        let mut x = Box::pin(self);
        unsafe {
            let xr = x.as_mut().get_unchecked_mut();
            match &mut xr.child {
                PopupOrSubsurface::Popup(x) => x.configure_listeners(),
                PopupOrSubsurface::Subsurface(x) => x.configure_listeners(),
            }
        }
        x
    }
}

fn damage_view(server: &Server, view: &mut View, full: bool) {
    for output in server.outputs.iter() {
        let o = output_coords(server, output);
        match &view.shell_surface {
            ShellView::Empty => (),
            ShellView::Xdg(v) => {
                let xdg_surface = v.xdgsurface.xdg_surface;
                xdg_surface_for_each_surface(xdg_surface, |s, x, y| {
                    let ox = o.x + view.x as f64 + x as f64;
                    let oy = o.y + view.y as f64 + y as f64;
                    damage_surface_at(output, s, ox, oy, full);
                });
            }
        }
    }
}

fn damage_surface_at(
    output: &Output,
    surface: &mut wl::wlr_surface,
    output_x: f64,
    output_y: f64,
    full: bool,
) {
    let sw = surface.current.width;
    let sh = surface.current.width;
    let mut area = scaled_box(output, output_x, output_y, sw as f64, sh as f64);

    if full {
        unsafe {
            wl::wlr_output_damage_add_box(output.damage, &mut area);
        }
    } else {
        unsafe {
            if wl::pixman_region32_not_empty(&mut surface.buffer_damage) != 0 {
                // figuring out the damage:
                // lets just do what sway wouild
                let scale = (*output.wlr_output).scale;

                let mut dmg = MaybeUninit::uninit();
                wl::pixman_region32_init(dmg.as_mut_ptr());
                let mut dmg = dmg.assume_init();

                wl::wlr_surface_get_effective_damage(surface, &mut dmg);
                wl::wlr_region_scale(&mut dmg, &mut dmg, scale);
                if scale.ceil() as i32 > surface.current.scale {
                    wl::wlr_region_expand(
                        &mut dmg,
                        &mut dmg,
                        scale.ceil() as i32 - surface.current.scale,
                    );
                }
                wl::pixman_region32_translate(&mut dmg, area.x, area.y);
                wl::wlr_output_damage_add(output.damage, &mut dmg);

                wl::pixman_region32_fini(&mut dmg);
            }
        }
    }
}

pub fn scaled_box(output: &Output, x: f64, y: f64, w: f64, h: f64) -> wl::wlr_box {
    let scale = unsafe { (*output.wlr_output).scale as f64 };

    wl::wlr_box {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
        width: (w * scale).round() as i32,
        height: (h * scale).round() as i32,
    }
}

pub fn output_coords(server: &Server, output: &Output) -> Point {
    let mut x = 0.0;
    let mut y = 0.0;
    unsafe {
        wl::wlr_output_layout_output_coords(
            server.output_layout,
            output.wlr_output,
            &mut x,
            &mut y,
        );
    }
    Point { x, y }
}
