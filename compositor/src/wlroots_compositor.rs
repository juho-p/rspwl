// unsafe {
//     lets goooo

use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr;

use wl_sys as wl;

use crate::wl_util::*;

pub type NodeId = u32;
pub type OutputId = u8;
pub type KeyboardId = u8;

pub struct Server {
    pub wl_display: *mut wl::wl_display,
    pub backend: *mut wl::wlr_backend,
    pub renderer: *mut wl::wlr_renderer,

    pub xdg_shell: *mut wl::wlr_xdg_shell,
    pub views: HashMap<NodeId, Pin<Box<View>>>,
    pub mru_node: Vec<NodeId>,

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

    pub next_node_id: NodeId,
}

impl Server {
    fn new_output_id(&self) -> OutputId {
        (1..255)
            .find(|id| self.outputs.iter().all(|output| output.id != *id))
            .expect("Too many outputs!")
    }

    fn new_node_id(&mut self) -> NodeId {
        let candidate = self.next_node_id;
        for _ in 0..1000 {
            if !self.views.contains_key(&candidate) {
                self.next_node_id = candidate + 1;
                return candidate;
            }
        }
        // This is paranoid and silly.
        // There's no way anyone ever opens 2^32 surfaces. And theres definetely no way they do
        // that AND have thousand surfaces AND they are are sequential.
        panic!("Could not get new node id. Either compositor is bugged or you are doing something *VERY* weird.");
    }
}

pub struct Output {
    id: OutputId,

    wlr_output: *mut wl::wlr_output,
    damage: *mut wl::wlr_output_damage,
    damage_frame: wl::wl_listener,
    destroy: Listener<(), OutputId>,

    _pin: PhantomPinned,
}

impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.damage_frame.link);
        }
    }
}

pub struct View {
    id: NodeId,

    xdg_surface: *mut wl::wlr_xdg_surface,

    map: wl::wl_listener,
    unmap: wl::wl_listener,
    commit: wl::wl_listener,
    destroy: wl::wl_listener,
    request_move: wl::wl_listener,
    request_resize: wl::wl_listener,
    new_subsurface: wl::wl_listener,
    new_popup: wl::wl_listener,
    mapped: bool,
    x: i32,
    y: i32,
    children: Vec<Pin<Box<ViewChild>>>,
    popups: Vec<Pin<Box<Popup>>>,

    _pin: PhantomPinned,
}

impl View {
    fn add_popup(&mut self, popup: *mut wl::wlr_xdg_popup) {
        self.popups.push(Popup::new(self.id, popup));
        unsafe {
            self.children.push(ViewChild::new(self.id, (*(*popup).base).surface));
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.map.link);
            wl::wl_list_remove(&mut self.unmap.link);
            wl::wl_list_remove(&mut self.commit.link);
            wl::wl_list_remove(&mut self.destroy.link);
            wl::wl_list_remove(&mut self.request_move.link);
            wl::wl_list_remove(&mut self.request_resize.link);
            wl::wl_list_remove(&mut self.new_subsurface.link);
            wl::wl_list_remove(&mut self.new_popup.link);
        }
    }
}

struct ViewChild {
    destroy: wl::wl_listener,
    commit: wl::wl_listener,
    new_subsurface: wl::wl_listener,

    surface: *mut wl::wlr_surface,

    parent_id: NodeId,
    _pin: PhantomPinned,
}
impl ViewChild {
    fn new(parent_id: NodeId, surface: *mut wl::wlr_surface) -> Pin<Box<Self>> {
        let mut child = Box::pin(ViewChild {
            parent_id,
            surface,
            destroy: new_wl_listener(Some(child_destroy)),
            commit: new_wl_listener(Some(child_commit)),
            new_subsurface: new_wl_listener(Some(child_new_subsurface)),

            _pin: PhantomPinned,
        });

        unsafe {
            let surface = &mut*surface;
            let x = child.as_mut().get_unchecked_mut();
            signal_add(&mut surface.events.destroy, &mut x.destroy);
            signal_add(&mut surface.events.commit, &mut x.commit);
            signal_add(&mut surface.events.new_subsurface, &mut x.new_subsurface);
        }

        child
    }
}
impl Drop for ViewChild {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.destroy.link);
            wl::wl_list_remove(&mut self.commit.link);
        }
    }
}
struct Popup {
    destroy: wl::wl_listener,
    new_popup: wl::wl_listener,
    popup: *mut wl::wlr_xdg_popup,

    parent_id: NodeId,
    _pin: PhantomPinned,
}
impl Popup {
    fn new(parent_id: NodeId, xdg_popup: *mut wl::wlr_xdg_popup) -> Pin<Box<Self>> {
        let mut popup = Box::pin(Popup {
            parent_id,
            popup: xdg_popup,
            destroy: new_wl_listener(Some(popup_destroy)),
            new_popup: new_wl_listener(Some(popup_new_popup)),

            _pin: PhantomPinned,
        });

        unsafe {
            let xdg_surface = &mut*(*xdg_popup).base;
            let x = popup.as_mut().get_unchecked_mut();
            signal_add(&mut xdg_surface.events.destroy, &mut x.destroy);
            signal_add(&mut xdg_surface.events.new_popup, &mut x.new_popup);
        }

        popup
    }
}
impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.destroy.link);
            wl::wl_list_remove(&mut self.new_popup.link);
        }
    }
}

struct Point {
    x: f64,
    y: f64,
}

pub struct Keyboard {
    device: *mut wl::wlr_input_device,
    modifiers: Listener<(), u8>,
    key: Listener<wl::wlr_event_keyboard_key, u8>,
    id: KeyboardId,
}

pub fn run_server() {
    unsafe {
        begin_adventure();
    }
}
unsafe fn begin_adventure() {
    // Safe territory ends here. Consider this as C, but be very careful about moving things. Do
    // not move the Server. It's not pinned, but that's just to keep things short. Be careful.

    wl::wlr_log_init(wl::wlr_log_importance_WLR_DEBUG, None);

    let wl_display = wl::wl_display_create();

    let backend = wl::wlr_backend_autocreate(wl_display);

    let renderer = wl::wlr_backend_get_renderer(backend);
    if !wl::wlr_renderer_init_wl_display(renderer, wl_display) {
        panic!("Failed to initialize display");
    }

    let _compositor = wl::wlr_compositor_create(wl_display, renderer);
    let _ddm = wl::wlr_data_device_manager_create(wl_display);

    let output_layout = wl::wlr_output_layout_create();

    let xdg_shell = wl::wlr_xdg_shell_create(wl_display);

    let cursor = wl::wlr_cursor_create();
    wl::wlr_cursor_attach_output_layout(cursor, output_layout);
    let cursor_mgr = wl::wlr_xcursor_manager_create(ptr::null(), 24);
    wl::wlr_xcursor_manager_load(cursor_mgr, 1.0);

    let socket = wl::wl_display_add_socket_auto(wl_display);
    if socket.is_null() {
        wl::wlr_backend_destroy(backend);
        panic!("Failed to create socket for the display");
    }

    let seat = wl::wlr_seat_create(wl_display, cstring!("seat0"));

    let mut server = Server {
        wl_display,
        backend,
        renderer,

        xdg_shell,
        views: HashMap::new(),
        mru_node: Vec::new(),

        cursor,
        cursor_mgr,

        cursor_motion: Listener::new(cursor_motion, ()),
        cursor_motion_absolute: Listener::new(cursor_motion_absolute, ()),
        cursor_button: Listener::new(cursor_button, ()),
        cursor_axis: Listener::new(cursor_axis, ()),
        cursor_frame: Listener::new(cursor_frame, ()),

        seat,
        new_input: Listener::new(handle_new_input, ()),
        request_cursor: Listener::new(handle_request_cursor, ()),
        request_set_selection: Listener::new(handle_request_set_selection, ()),
        keyboards: Vec::new(),

        outputs: Vec::new(),
        output_layout,

        new_xdg_surface: Listener::new(new_xdg_surface, ()),
        new_output: Listener::new(new_output, ()),

        next_node_id: 1,
    };

    listen_server_signal(
        &mut (*server.backend).events.new_output,
        &mut server.new_output,
    );
    listen_server_signal(
        &mut (*server.xdg_shell).events.new_surface,
        &mut server.new_xdg_surface,
    );
    listen_server_signal(
        &mut (*server.cursor).events.motion,
        &mut server.cursor_motion,
    );
    listen_server_signal(
        &mut (*server.cursor).events.motion_absolute,
        &mut server.cursor_motion_absolute,
    );
    listen_server_signal(
        &mut (*server.cursor).events.button,
        &mut server.cursor_button,
    );
    listen_server_signal(&mut (*server.cursor).events.axis, &mut server.cursor_axis);
    listen_server_signal(&mut (*server.cursor).events.frame, &mut server.cursor_frame);
    listen_server_signal(
        &mut (*server.backend).events.new_input,
        &mut server.new_input,
    );
    listen_server_signal(
        &mut (*server.seat).events.request_set_cursor,
        &mut server.request_cursor,
    );
    listen_server_signal(
        &mut (*server.seat).events.request_set_selection,
        &mut server.request_set_selection,
    );

    SERVER_GLOBAL = &mut server;

    let socket = wl::wl_display_add_socket_auto(server.wl_display);
    if socket.is_null() {
        wl::wlr_backend_destroy(server.backend);
    } else {
        if !wl::wlr_backend_start(server.backend) {
            wl::wlr_backend_destroy(server.backend);
            wl::wl_display_destroy(server.wl_display);
        } else {
            debug!("Run display");
            wl::wl_display_run(server.wl_display);

            wl::wl_display_destroy_clients(server.wl_display);
            wl::wl_display_destroy(server.wl_display);
        }
    }

    SERVER_GLOBAL = ptr::null_mut();
}

struct ListenerWrapper(wl::wl_listener);
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

pub struct Listener<Data, Ctx: Copy> {
    listener: ListenerWrapper,
    ctx: Ctx,
    f: fn(&mut Server, &mut Data, Ctx),
}

impl<Data, Ctx: Copy> Listener<Data, Ctx> {
    fn new(f: fn(&mut Server, &mut Data, Ctx) -> (), ctx: Ctx) -> Self {
        Listener {
            listener: ListenerWrapper(new_wl_listener(Some(server_notify::<Data, Ctx>))),
            ctx,
            f,
        }
    }
}

// After this call, do NOT move the listener (yeah, yeah, should pin. maybe later)
unsafe fn listen_server_signal<Data, Ctx: Copy>(
    signal: &mut wl::wl_signal,
    listener: &mut Listener<Data, Ctx>,
) {
    signal_add(signal, &mut listener.listener.0);
}

static mut SERVER_GLOBAL: *mut Server = ptr::null_mut();

unsafe fn server_ptr() -> *mut Server {
    if SERVER_GLOBAL.is_null() {
        panic!("BUG! Trying to use uninitialized server");
    } else {
        SERVER_GLOBAL
    }
}
unsafe extern "C" fn server_notify<Data, Ctx: Copy>(
    wl_listener: *mut wl::wl_listener,
    data: *mut c_void,
) {
    let p = container_of!(Listener<Data, Ctx>, listener, wl_listener);
    ((*p).f)(&mut *server_ptr(), &mut *(data as *mut Data), (*p).ctx);
}

// ---

fn new_output(server: &mut Server, wlr_output: &mut wl::wlr_output, _: ()) {
    unsafe {
        if wl::wl_list_empty(&(*wlr_output).modes) == 0 {
            // only try to set mode if there are any
            let mode = wl::wlr_output_preferred_mode(wlr_output);
            wl::wlr_output_set_mode(wlr_output, mode);
            wl::wlr_output_enable(wlr_output, true);
            if !wl::wlr_output_commit(wlr_output) {
                eprintln!("Failed to set mode");
                return;
            }
        }
    }

    let id = server.new_output_id();
    let damage = unsafe { wl::wlr_output_damage_create(wlr_output) };
    let mut output = Box::pin(Output {
        id,
        wlr_output,
        damage,
        damage_frame: new_wl_listener(Some(damage_handle_frame)),
        destroy: Listener::new(output_destroy, id),

        _pin: PhantomPinned,
    });
    unsafe {
        let x = output.as_mut().get_unchecked_mut();
        signal_add(&mut (*damage).events.frame, &mut &mut x.damage_frame);
        listen_server_signal(&mut (*wlr_output).events.destroy, &mut x.destroy);
        wl::wlr_output_layout_add_auto(server.output_layout, wlr_output);
    }

    server.outputs.push(output);
}

fn new_xdg_surface(server: &mut Server, xdg_surface: &mut wl::wlr_xdg_surface, _: ()) {
    if xdg_surface.role != wl::wlr_xdg_surface_role_WLR_XDG_SURFACE_ROLE_TOPLEVEL {
        info!("xdg-shell popup");
        return;
    }

    let id = server.new_node_id();

    let mut view = Box::pin(View {
        id,
        xdg_surface,
        map: new_wl_listener(Some(xdg_surface_map)),
        unmap: new_wl_listener(Some(xdg_surface_unmap)),
        commit: new_wl_listener(Some(surface_commit)),
        destroy: new_wl_listener(Some(xdg_surface_destroy)),
        request_move: new_wl_listener(Some(xdg_surface_request_move)),
        request_resize: new_wl_listener(Some(xdg_surface_request_resize)),
        new_subsurface: new_wl_listener(Some(new_subsurface)),
        new_popup: new_wl_listener(Some(new_popup)),
        mapped: false,
        x: 70,
        y: 5,
        children: Vec::new(),
        popups: Vec::new(),

        _pin: PhantomPinned,
    });

    unsafe {
        let x = view.as_mut().get_unchecked_mut();

        let toplevel = &mut *xdg_surface.__bindgen_anon_1.toplevel;
        let wev = &mut (*xdg_surface.surface).events;
        let tev = &mut toplevel.events;

        info!("xdg-shell toplevel {:?}: {:?}", CStr::from_ptr(toplevel.app_id), CStr::from_ptr(toplevel.title));

        // TODO XXX ?? keep pinging more later
        wl::wlr_xdg_surface_ping(xdg_surface);

        signal_add(&mut xdg_surface.events.map, &mut x.map);
        signal_add(&mut xdg_surface.events.unmap, &mut x.unmap);
        signal_add(&mut wev.commit, &mut x.commit);
        signal_add(&mut xdg_surface.events.destroy, &mut x.destroy);
        signal_add(&mut tev.request_move, &mut x.request_move);
        signal_add(&mut tev.request_resize, &mut x.request_resize);
        signal_add(&mut wev.new_subsurface, &mut x.new_subsurface);
        signal_add(&mut xdg_surface.events.new_popup, &mut x.new_popup);
    }

    server.views.insert(id, view);
    server.mru_node.push(id);

    invalidate_everything(server);
}

unsafe extern "C" fn damage_handle_frame(listener: *mut wl::wl_listener, _: *mut c_void) {
    let output = &mut *container_of!(Output, damage_frame, listener);

    if !(*output.wlr_output).enabled {
        info!("Output disabled");
        return;
    }

    let mut needs_frame = false;
    let mut damage_region = MaybeUninit::uninit();
    wl::pixman_region32_init(damage_region.as_mut_ptr());
    let mut damage_region = damage_region.assume_init();

    if !wl::wlr_output_damage_attach_render(output.damage, &mut needs_frame, &mut damage_region) {
        return;
    }

    if needs_frame {
        render(output, &mut damage_region);
        wl::wlr_output_commit(output.wlr_output);
    } else {
        wl::wlr_output_rollback(output.wlr_output);
    }
    wl::pixman_region32_fini(&mut damage_region);
}

fn output_coords(server: &Server, output: &Output) -> Point {
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

unsafe fn render_surface(
    surface: *mut wl::wlr_surface,
    sx: i32,
    sy: i32,
    server: &Server,
    output: &Output,
    view: &View,
    when: *const wl::timespec,
    output_damage: *mut wl::pixman_region32,
) {
    let texture = wl::wlr_surface_get_texture(surface);
    if texture.is_null() {
        return;
    }

    let mut src_box = MaybeUninit::uninit();
    wl::wlr_surface_get_buffer_source_box(surface, src_box.as_mut_ptr());
    let src_box = src_box.assume_init();

    let Point {
        x: mut ox,
        y: mut oy,
    } = output_coords(server, output);
    ox += (view.x + sx) as f64;
    oy += (view.y + sy) as f64;

    let sw = (*surface).current.width;
    let sh = (*surface).current.height;

    let dst_box = wl::wlr_box {
        x: ox as i32,
        y: oy as i32,
        width: sw,
        height: sh,
    };

    let mut damage = MaybeUninit::uninit();
    wl::pixman_region32_init(damage.as_mut_ptr());
    let mut damage = damage.assume_init();
    wl::pixman_region32_union_rect(
        &mut damage,
        &mut damage,
        dst_box.x,
        dst_box.y,
        dst_box.width as u32,
        dst_box.height as u32,
    );
    // Get the output damage that overlaps the surface
    wl::pixman_region32_intersect(&mut damage, &mut damage, output_damage);
    if wl::pixman_region32_not_empty(&mut damage) != 0 {
        let viewbox = scaled_box(output, ox, oy, sw as f64, sh as f64);

        let mut matrix = [0f32; 9];
        let surface_transform = wl::wlr_output_transform_invert((*surface).current.transform);
        wl::wlr_matrix_project_box(
            matrix.as_mut_ptr(),
            &viewbox,
            surface_transform,
            0f32,
            (*output.wlr_output).transform_matrix.as_ptr(),
        );

        let mut nrects = 0;
        let rects = wl::pixman_region32_rectangles(&mut damage, &mut nrects);
        let opacity = 1.0;
        let mut ow = 0;
        let mut oh = 0;

        for idx in 0..nrects {
            let rect = &*rects.offset(idx as isize);

            // Here's what we do: Scissor the renderning and then just try to do the whole surface.
            // This is what Sway does and it's probably right since those folks know how to use
            // wlroots.
            let mut scissor_box = wl::wlr_box {
                x: rect.x1,
                y: rect.y1,
                width: rect.x2 - rect.x1,
                height: rect.y2 - rect.y1,
            };
            debug!("- RECT {:?}", scissor_box);

            wl::wlr_output_transformed_resolution(output.wlr_output, &mut ow, &mut oh);
            let transform = wl::wlr_output_transform_invert((*output.wlr_output).transform);
            wl::wlr_box_transform(&mut scissor_box, &mut scissor_box, transform, ow, oh);

            wl::wlr_renderer_scissor(server.renderer, &mut scissor_box);

            // In addition to this, Sway would configure opengl texture scaling filter. We might
            // want to do that as well.
            wl::wlr_render_subtexture_with_matrix(
                server.renderer,
                texture,
                &src_box,
                matrix.as_mut_ptr(),
                opacity,
            );
            //wl::wlr_render_texture_with_matrix(rdata.renderer, texture, matrix.as_mut_ptr(), 1.0);

            // DEBUG
            // let mut c = [1.0, 0.0, 0.0, 1.0];
            // wl::wlr_render_rect(
            //     rdata.renderer,
            //     &mut wl::wlr_box {
            //         x: scissor_box.x,
            //         y: scissor_box.y,
            //         width: 5,
            //         height: scissor_box.height,
            //     },
            //     c.as_mut_ptr(),
            //     (*output.wlr_output).transform_matrix.as_mut_ptr(),
            // );
            // wl::wlr_render_rect(
            //     rdata.renderer,
            //     &mut wl::wlr_box {
            //         x: scissor_box.x,
            //         y: scissor_box.y,
            //         width: scissor_box.width,
            //         height: 5,
            //     },
            //     c.as_mut_ptr(),
            //     (*output.wlr_output).transform_matrix.as_mut_ptr(),
            // );
        }
    }
    wl::pixman_region32_fini(&mut damage);

    wl::wlr_surface_send_frame_done(surface, when);
}

unsafe fn render(output: &mut Output, damage: *mut wl::pixman_region32) {
    let server = &*server_ptr();
    let renderer = server.renderer;

    let mut now = MaybeUninit::uninit();
    wl::clock_gettime(wl::CLOCK_MONOTONIC as i32, now.as_mut_ptr());
    let now = now.assume_init();

    let mut width = 0;
    let mut height = 0;
    wl::wlr_output_effective_resolution(output.wlr_output, &mut width, &mut height);
    wl::wlr_renderer_begin(renderer, width as u32, height as u32);

    //let color = [0.3, 0.3, 0.3, 1.0];
    //wl::wlr_renderer_clear(renderer, color.as_ptr());

    debug!("BEGIN RENDER");
    for view in server.views.values().filter(|x| x.mapped) {
        xdg_surface_for_each_surface(view.xdg_surface, |s, x, y| {
            debug!("-surface");
            render_surface(
                s,
                x,
                y,
                server,
                output,
                view,
                &now,
                damage
            );
        })
        // wl::wlr_xdg_surface_for_each_surface(
        //     view.xdg_surface,
        //     Some(render_surface),
        //     &mut rdata as *mut RenderData as *mut c_void,
        // );
    }
    wl::wlr_output_render_software_cursors(output.wlr_output, ptr::null_mut());
    debug!("END RENDER");

    wl::wlr_renderer_end(renderer);
}

fn output_destroy(server: &mut Server, _: &mut (), id: OutputId) {
    // TODO test if it fine that this destroys the damage as well
    server.outputs.retain(|x| x.id != id);
}

fn cursor_motion(server: &mut Server, motion: &mut wl::wlr_event_pointer_motion, _: ()) {
    unsafe {
        wl::wlr_cursor_move(server.cursor, motion.device, motion.delta_x, motion.delta_y);
        handle_cursor_move(server, motion.time_msec);
    }
}
fn cursor_motion_absolute(
    server: &mut Server,
    motion: &mut wl::wlr_event_pointer_motion_absolute,
    _: (),
) {
    unsafe {
        wl::wlr_cursor_warp_absolute(server.cursor, motion.device, motion.x, motion.y);
        handle_cursor_move(server, motion.time_msec);
    }
}

fn cursor_pos(server: &Server) -> Point {
    unsafe {
        Point {
            x: (*server.cursor).x,
            y: (*server.cursor).y,
        }
    }
}
unsafe fn handle_cursor_move(server: &mut Server, time: u32) {
    if let Some((_, surface, p)) = find_view(server, cursor_pos(server)) {
        // Enter is kind of wrong after the first time, but wlroots promises to disregard those so
        // no matter
        wl::wlr_seat_pointer_notify_enter(server.seat, surface, p.x, p.y);
        wl::wlr_seat_pointer_notify_motion(server.seat, time, p.x, p.y);
    } else {
        // TODO Could figure out if it's ok to just call this all the time
        wl::wlr_xcursor_manager_set_cursor_image(
            server.cursor_mgr,
            cstring!("left_ptr"),
            server.cursor,
        );
    }
}

fn cursor_button(server: &mut Server, event: &mut wl::wlr_event_pointer_button, _: ()) {
    unsafe {
        wl::wlr_seat_pointer_notify_button(server.seat, event.time_msec, event.button, event.state);
    }

    if event.state == wl::wlr_button_state_WLR_BUTTON_RELEASED {
        // TODO release button here (resize drag, etc)
    } else {
        if let Some((view, surface, _)) = find_view(server, cursor_pos(server)) {
            let view_id = view.id;
            let toplevel = view.xdg_surface;
            info!("clicked view {}", view_id);
            unsafe {
                focus(server, view_id, toplevel, surface);
            }
        } else {
            info!("clicked outside view");
        }
    }
}
fn cursor_axis(server: &mut Server, event: &mut wl::wlr_event_pointer_axis, _: ()) {
    // mouse wheel
    unsafe {
        wl::wlr_seat_pointer_notify_axis(
            server.seat,
            event.time_msec,
            event.orientation,
            event.delta,
            event.delta_discrete,
            event.source,
        );
    }
}
fn cursor_frame(server: &mut Server, _: &mut (), _: ()) {
    unsafe {
        wl::wlr_seat_pointer_notify_frame(server.seat);
    }
}

unsafe fn focus(
    server: &mut Server,
    view_id: NodeId,
    toplevel: *mut wl::wlr_xdg_surface,
    surface: *mut wl::wlr_surface,
) {
    println!("set focus");
    let prev_surface = (*server.seat).keyboard_state.focused_surface;
    if prev_surface == surface {
        return;
    }
    if !prev_surface.is_null() {
        let prev = wl::wlr_xdg_surface_from_wlr_surface(prev_surface);
        wl::wlr_xdg_toplevel_set_activated(prev, false);
    }

    let keyboard = &mut *wl::wlr_seat_get_keyboard(server.seat);
    wl::wlr_xdg_toplevel_set_activated(toplevel, false);
    wl::wlr_seat_keyboard_notify_enter(
        server.seat,
        surface,
        keyboard.keycodes.as_mut_ptr(),
        keyboard.num_keycodes,
        &mut keyboard.modifiers,
    );

    remove_id(&mut server.mru_node, view_id);
    server.mru_node.push(view_id);
}

fn handle_new_input(server: &mut Server, device: &mut wl::wlr_input_device, _: ()) {
    // New input device available

    if device.type_ == wl::wlr_input_device_type_WLR_INPUT_DEVICE_KEYBOARD {
        let id = (1..255).find(|i| server.keyboards.iter().all(|x| x.id != *i));
        if let Some(id) = id {
            let mut keyboard = Box::pin(Keyboard {
                device,
                modifiers: Listener::new(handle_modifiers, id),
                key: Listener::new(handle_key, id),
                id,
            });
            unsafe {
                let kb = keyboard.as_mut().get_unchecked_mut();
                let wlr_keyboard = &mut (*device.__bindgen_anon_1.keyboard);
                let events = &mut wlr_keyboard.events;
                listen_server_signal(&mut events.modifiers, &mut kb.modifiers);
                listen_server_signal(&mut events.key, &mut kb.key);

                let context = wl::xkb_context_new(0);
                let keymap = wl::xkb_keymap_new_from_names(context, ptr::null(), 0);

                // TODO: these should actually come from some kind of keyboard settings (and not
                // hard coded)
                wl::wlr_keyboard_set_keymap(wlr_keyboard, keymap);
                wl::xkb_keymap_unref(keymap);
                wl::xkb_context_unref(context);
                wl::wlr_keyboard_set_repeat_info(wlr_keyboard, 25, 600);

                // set the keyboard in here as well
                println!("Set keybrd");
                wl::wlr_seat_set_keyboard(server.seat, device);
            }

            server.keyboards.push(keyboard);
        } else {
            eprintln!("Can't add keyboard. WHY DO YOU HAVE SO MANY KEYBOARDS?");
        }
    } else if device.type_ == wl::wlr_input_device_type_WLR_INPUT_DEVICE_POINTER {
        unsafe {
            wl::wlr_cursor_attach_input_device(server.cursor, device);
        }
    }

    // why wouldn't you have all the caps
    // TODO could still do this right
    let caps = wl::wl_seat_capability_WL_SEAT_CAPABILITY_POINTER
        | wl::wl_seat_capability_WL_SEAT_CAPABILITY_KEYBOARD;

    //if server.keyboards.is_empty() {
    //}

    unsafe {
        wl::wlr_seat_set_capabilities(server.seat, caps);
    }
}

fn handle_request_cursor(
    server: &mut Server,
    event: &mut wl::wlr_seat_pointer_request_set_cursor_event,
    _: (),
) {
    // Client provides cursor image
    // lets do whatever tinywl would do here
    unsafe {
        let focused_client = (*server.seat).pointer_state.focused_client;
        if focused_client == event.seat_client {
            wl::wlr_cursor_set_surface(
                server.cursor,
                event.surface,
                event.hotspot_x,
                event.hotspot_y,
            );
        }
    }
}

fn handle_request_set_selection(
    server: &mut Server,
    event: &mut wl::wlr_seat_request_set_selection_event,
    _: (),
) {
    // Client wants to set selection (copy something)
    // NOTE: could decide not to grant request for non-focused applications
    unsafe {
        wl::wlr_seat_set_selection(server.seat, event.source, event.serial);
    }
}

fn handle_modifiers(server: &mut Server, _: &mut (), id: u8) {
    if let Some(keyboard) = server.keyboards.iter().find(|x| x.id == id) {
        let device = keyboard.device;
        unsafe {
            println!("Set keybrd (in mod)");
            wl::wlr_seat_set_keyboard(server.seat, device);
            let keyboard = (*device).__bindgen_anon_1.keyboard;
            wl::wlr_seat_keyboard_notify_modifiers(server.seat, &mut (*keyboard).modifiers);
        }
    }
}

fn handle_key(server: &mut Server, event: &mut wl::wlr_event_keyboard_key, id: u8) {
    if let Some(keyboard) = server.keyboards.iter().find(|x| x.id == id) {
        // TODO handle meaningful keys here. like tinywl would do:
        // /* Translate libinput keycode -> xkbcommon */
        // uint32_t keycode = event->keycode + 8;
        // /* Get a list of keysyms based on the keymap for this keyboard */
        // const xkb_keysym_t *syms;
        // int nsyms = xkb_state_key_get_syms(
        //         keyboard->device->keyboard->xkb_state, keycode, &syms);

        // bool handled = false;
        // uint32_t modifiers = wlr_keyboard_get_modifiers(keyboard->device->keyboard);
        // if ((modifiers & WLR_MODIFIER_ALT) && event->state == WL_KEYBOARD_KEY_STATE_PRESSED) {
        //     /* If alt is held down and this button was _pressed_, we attempt to
        //      * process it as a compositor keybinding. */
        //     for (int i = 0; i < nsyms; i++) {
        //         handled = handle_keybinding(server, syms[i]);
        //     }
        // }

        // NOTE: when not handled by some shortcut system, pass it to seat
        unsafe {
            println!("Handle key");
            wl::wlr_seat_set_keyboard(server.seat, keyboard.device);
            wl::wlr_seat_keyboard_notify_key(
                server.seat,
                event.time_msec,
                event.keycode,
                event.state,
            );
        }
    }
}

fn find_view(
    server: &Server,
    pos: Point,
) -> Option<(&Pin<Box<View>>, *mut wl::wlr_surface, Point)> {
    // TODO should do in mru order
    server.views.values().find_map(|view| {
        let sx = pos.x - view.x as f64;
        let sy = pos.y - view.y as f64;
        let mut sub = Point { x: 0.0, y: 0.0 };
        let surface = unsafe {
            wl::wlr_xdg_surface_surface_at(view.xdg_surface, sx, sy, &mut sub.x, &mut sub.y)
        };
        if surface.is_null() {
            None
        } else {
            Some((view, surface, sub))
        }
    })
}

fn invalidate_everything(server: &Server) {
    for output in server.outputs.iter() {
        unsafe {
            wl::wlr_output_damage_add_whole(output.damage);
        }
    }
}

macro_rules! view {
    ($listener:ident, $field:tt) => {
        &mut *container_of!(View, $field, $listener)
    };
}

unsafe extern "C" fn xdg_surface_map(listener: *mut wl::wl_listener, _: *mut c_void) {
    let view = view!(listener, map);
    view.mapped = true;
    focus(
        &mut *server_ptr(),
        view.id,
        view.xdg_surface,
        (*view.xdg_surface).surface,
    );
    damage_view(&*server_ptr(), view, true);
}
unsafe extern "C" fn xdg_surface_unmap(listener: *mut wl::wl_listener, _: *mut c_void) {
    let view = view!(listener, unmap);
    view.mapped = false;
    // should do damage here
}
unsafe extern "C" fn surface_commit(listener: *mut wl::wl_listener, _: *mut c_void) {
    let view = view!(listener, commit);
    info!("commit");
    damage_view(&*server_ptr(), view, false);
}
unsafe extern "C" fn new_subsurface(listener: *mut wl::wl_listener, data: *mut c_void) {
    let view = view!(listener, new_subsurface);
    info!("New subsurface");

    let subsurface = data as *mut wl::wlr_subsurface;

    let id = view.id;
    view.children.push(ViewChild::new(id, (*subsurface).surface));
}
unsafe extern "C" fn new_popup(listener: *mut wl::wl_listener, data: *mut c_void) {
    let view = view!(listener, new_popup);
    info!("New popup");

    let popup = data as *mut wl::wlr_xdg_popup;

    view.add_popup(popup);
}
unsafe extern "C" fn xdg_surface_destroy(listener: *mut wl::wl_listener, _: *mut c_void) {
    let id = view!(listener, destroy).id;
    let server = &mut *server_ptr();
    server.views.remove(&id);
    remove_id(&mut server.mru_node, id);
}
fn remove_id(ids: &mut Vec<NodeId>, id: NodeId) {
    let mru_idx = ids
        .iter()
        .rposition(|x| *x == id)
        .expect("BUG: view missing from mru vec");
    ids.remove(mru_idx);
}

unsafe extern "C" fn xdg_surface_request_move(listener: *mut wl::wl_listener, _: *mut c_void) {
    let id = view!(listener, request_move).id;

    println!("view {} requested move but we don't care", id);
}
unsafe extern "C" fn xdg_surface_request_resize(listener: *mut wl::wl_listener, _: *mut c_void) {
    let id = view!(listener, request_resize).id;
    println!("view {} requested resize but we don't care", id);
}

unsafe extern "C" fn child_destroy(listener: *mut wl::wl_listener, _: *mut c_void) {
    let child = &*container_of!(ViewChild, destroy, listener);
    if let Some(view) = (*server_ptr()).views.get_mut(&child.parent_id) {
        if let Some(index) = view.children.iter().position(|x| x.surface == child.surface) {
            view.as_mut().get_unchecked_mut().children.swap_remove(index);
        } else {
            warn!("subsurface_destroy: Child is missing");
        }
    } else {
        warn!("subsurface_destroy: View is missing");
    }
}
unsafe extern "C" fn child_commit(listener: *mut wl::wl_listener, _: *mut c_void) {
    let _child = &*container_of!(ViewChild, commit, listener);
    // TODO bad XXX
    warn!("Invalidate all for nothing");
    invalidate_everything(&*server_ptr());
}
unsafe extern "C" fn child_new_subsurface(listener: *mut wl::wl_listener, data: *mut c_void) {
    // TODO should unify this surface handling stuff
    let subsurface = data as *mut wl::wlr_subsurface;
    let child = &*container_of!(ViewChild, new_subsurface, listener);
    if let Some(view) = (*server_ptr()).views.get_mut(&child.parent_id) {
        view.as_mut().get_unchecked_mut().children.push(ViewChild::new(child.parent_id, (*subsurface).surface));
    } else {
        warn!("subsurface_new_subsurface: View is missing");
    }
}

unsafe extern "C" fn popup_destroy(listener: *mut wl::wl_listener, _: *mut c_void) {
    let popup = &*container_of!(Popup, destroy, listener);
    info!("Destroy popup");
    if let Some(view) = (*server_ptr()).views.get_mut(&popup.parent_id) {
        if let Some(index) = view.popups.iter().position(|x| x.popup == popup.popup) {
            view.as_mut().get_unchecked_mut().popups.swap_remove(index);
        } else {
            warn!("subsurface_destroy: Child is missing");
        }
    } else {
        warn!("subsurface_destroy: View is missing");
    }
}
unsafe extern "C" fn popup_new_popup(listener: *mut wl::wl_listener, data: *mut c_void) {
    let new_xdg_popup = data as *mut wl::wlr_xdg_popup;
    info!("New popup  from popup");
    let popup = &*container_of!(Popup, new_popup, listener);
    if let Some(view) = (*server_ptr()).views.get_mut(&popup.parent_id) {
        view.as_mut().get_unchecked_mut().add_popup(new_xdg_popup);
    } else {
        warn!("popup_new_popup: View is missing");
    }
}

fn scaled_box(output: &Output, x: f64, y: f64, w: f64, h: f64) -> wl::wlr_box {
    let scale = unsafe { (*output.wlr_output).scale as f64 };

    wl::wlr_box {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
        width: (w * scale).round() as i32,
        height: (h * scale).round() as i32,
    }
}

fn damage_view(server: &Server, view: &mut View, full: bool) {
    for output in server.outputs.iter() {
        let o = output_coords(server, output);
        xdg_surface_for_each_surface(view.xdg_surface, |s, x, y| {
            let ox = o.x + view.x as f64 + x as f64;
            let oy = o.y + view.y as f64 + y as f64;
            let sw = (*s).current.width;
            let sh = (*s).current.width;
            let mut area = scaled_box(output, ox, oy, sw as f64, sh as f64);

            if full {
                unsafe {
                    wl::wlr_output_damage_add_box(output.damage, &mut area);
                }
            } else {
                unsafe {
                    if wl::pixman_region32_not_empty(&mut s.buffer_damage) != 0 {
                        // figuring out the damage:
                        // lets just do what sway wouild
                        let scale = (*output.wlr_output).scale;

                        let mut dmg = MaybeUninit::uninit();
                        wl::pixman_region32_init(dmg.as_mut_ptr());
                        let mut dmg = dmg.assume_init();

                        wl::wlr_surface_get_effective_damage(s, &mut dmg);
                        wl::wlr_region_scale(&mut dmg, &mut dmg, scale);
                        if scale.ceil() as i32 > s.current.scale {
                            wl::wlr_region_expand(
                                &mut dmg,
                                &mut dmg,
                                scale.ceil() as i32 - s.current.scale,
                            );
                        }
                        wl::pixman_region32_translate(&mut dmg, area.x, area.y);
                        wl::wlr_output_damage_add(output.damage, &mut dmg);

                        wl::pixman_region32_fini(&mut dmg);
                    }
                }
                // if (pixman_region32_not_empty(&surface->buffer_damage)) {
                // 	pixman_region32_t damage;
                // 	pixman_region32_init(&damage);
                // 	wlr_surface_get_effective_damage(surface, &damage);
                // 	wlr_region_scale(&damage, &damage, output->wlr_output->scale);
                // 	if (ceil(output->wlr_output->scale) > surface->current.scale) {
                // 		// When scaling up a surface, it'll become blurry so we need to
                // 		// expand the damage region
                // 		wlr_region_expand(&damage, &damage,
                // 			ceil(output->wlr_output->scale) - surface->current.scale);
                // 	}
                // 	pixman_region32_translate(&damage, box.x, box.y);
                // 	wlr_output_damage_add(output->damage, &damage);
                // 	pixman_region32_fini(&damage);
                // }
            }
        });
    }
}
