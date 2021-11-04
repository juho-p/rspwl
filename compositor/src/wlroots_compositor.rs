use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::os::raw::c_int;
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
    pub nodes: HashMap<NodeId, Pin<Box<Node>>>,
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
            if !self.nodes.contains_key(&candidate) {
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

pub struct Node {
    id: NodeId,

    xdg_surface: *mut wl::wlr_xdg_surface,

    map: wl::wl_listener,
    unmap: wl::wl_listener,
    commit: wl::wl_listener,
    destroy: wl::wl_listener,
    request_move: wl::wl_listener,
    request_resize: wl::wl_listener,
    mapped: bool,
    x: i32,
    y: i32,

    _pin: PhantomPinned,
}

struct Point {
    x: f64,
    y: f64,
}

impl Drop for Node {
    fn drop(&mut self) {
        unsafe {
            wl::wl_list_remove(&mut self.map.link);
            wl::wl_list_remove(&mut self.unmap.link);
            wl::wl_list_remove(&mut self.commit.link);
            wl::wl_list_remove(&mut self.destroy.link);
            wl::wl_list_remove(&mut self.request_move.link);
            wl::wl_list_remove(&mut self.request_resize.link);
        }
    }
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
        nodes: HashMap::new(),
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

    println!("DO SOCKET");
    let socket = wl::wl_display_add_socket_auto(server.wl_display);
    let sockcstr = CStr::from_ptr(socket);
    println!("SOCKET {:?}", sockcstr);
    if socket.is_null() {
        wl::wlr_backend_destroy(server.backend);
    } else {
        println!("DO START");
        if !wl::wlr_backend_start(server.backend) {
            wl::wlr_backend_destroy(server.backend);
            wl::wl_display_destroy(server.wl_display);
        } else {
            //std::env::set_var("WAYLAND_DISPLAY", socket)

            // TODO: should setup some nice wlr logging for Rust (to get the safest WLR logging EVER
            //wl::wlr_log(WLR_INFO, "Running Wayland compositor on WAYLAND_DISPLAY=%s",
            //        socket);
            println!("RUN IT!");
            wl::wl_display_run(server.wl_display);

            wl::wl_display_destroy_clients(server.wl_display);
            wl::wl_display_destroy(server.wl_display);
        }
    }

    // TODO go thru, is all ok:

    // server.cursor_motion.notify = server_cursor_motion;
    // wl_signal_add(&server.cursor->events.motion, &server.cursor_motion);
    // server.cursor_motion_absolute.notify = server_cursor_motion_absolute;
    // wl_signal_add(&server.cursor->events.motion_absolute,
    //         &server.cursor_motion_absolute);
    // server.cursor_button.notify = server_cursor_button;
    // wl_signal_add(&server.cursor->events.button, &server.cursor_button);
    // server.cursor_axis.notify = server_cursor_axis;
    // wl_signal_add(&server.cursor->events.axis, &server.cursor_axis);
    // server.cursor_frame.notify = server_cursor_frame;
    // wl_signal_add(&server.cursor->events.frame, &server.cursor_frame);
    //
    // server.new_input.notify = server_new_input;
    // wl_signal_add(&server.backend->events.new_input, &server.new_input);
    // server.request_cursor.notify = seat_request_cursor;
    // wl_signal_add(&server.seat->events.request_set_cursor,
    //         &server.request_cursor);
    // server.request_set_selection.notify = seat_request_set_selection;
    // wl_signal_add(&server.seat->events.request_set_selection,
    //         &server.request_set_selection);

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
        println!("Skip non-toplevel");
        return;
    }

    let id = server.new_node_id();

    let mut node = Box::pin(Node {
        id,
        xdg_surface,
        map: new_wl_listener(Some(xdg_surface_map)),
        unmap: new_wl_listener(Some(xdg_surface_unmap)),
        commit: new_wl_listener(Some(xdg_surface_commit)),
        destroy: new_wl_listener(Some(xdg_surface_destroy)),
        request_move: new_wl_listener(Some(xdg_surface_request_move)),
        request_resize: new_wl_listener(Some(xdg_surface_request_resize)),
        mapped: false,
        x: 70,
        y: 5,

        _pin: PhantomPinned,
    });

    unsafe {
        let x = node.as_mut().get_unchecked_mut();

        let ev = &mut (*xdg_surface).events;
        let wev = &mut (*(*xdg_surface).surface).events;
        let toplevel = (*xdg_surface).__bindgen_anon_1.toplevel; // very cool, bindgen, thanks
        let tev = &mut (*toplevel).events;

        signal_add(&mut ev.map, &mut x.map);
        signal_add(&mut ev.unmap, &mut x.unmap);
        signal_add(&mut wev.commit, &mut x.commit);
        signal_add(&mut ev.destroy, &mut x.destroy);
        signal_add(&mut tev.request_move, &mut x.request_move);
        signal_add(&mut tev.request_resize, &mut x.request_resize);
    }

    server.nodes.insert(id, node);
    server.mru_node.push(id);
}

unsafe extern "C" fn damage_handle_frame(listener: *mut wl::wl_listener, _: *mut c_void) {
    let output = &mut *container_of!(Output, damage_frame, listener);

    if !(*output.wlr_output).enabled {
        println!("XXX output disabled"); // TODO
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
        render(output);
        wl::wlr_output_commit(output.wlr_output);
    } else {
        wl::wlr_output_rollback(output.wlr_output);
    }
    wl::pixman_region32_fini(&mut damage_region);
}

struct RenderData {
    server: *const Server,
    output: *mut wl::wlr_output,
    node: *const Node,
    renderer: *mut wl::wlr_renderer,
    when: *const wl::timespec,
}

unsafe extern "C" fn render_surface(
    surface: *mut wl::wlr_surface,
    sx: c_int,
    sy: c_int,
    data: *mut c_void,
) {
    let rdata = &*(data as *const RenderData);
    let node = &*rdata.node;
    let server = &*rdata.server;

    let texture = wl::wlr_surface_get_texture(surface);
    if texture.is_null() {
        return;
    }

    let mut ox = 0.0;
    let mut oy = 0.0;
    wl::wlr_output_layout_output_coords(server.output_layout, rdata.output, &mut ox, &mut oy);
    ox += (node.x + sx) as f64;
    oy += (node.y + sy) as f64;

    let scale = (*rdata.output).scale as f64;
    let viewbox = wl::wlr_box {
        x: (ox * scale) as i32,
        y: (oy * scale) as i32,
        width: ((*surface).current.width as f64 * scale) as i32,
        height: ((*surface).current.height as f64 * scale) as i32,
    };

    let mut matrix = [0f32; 9];
    let transform = wl::wlr_output_transform_invert((*surface).current.transform);
    wl::wlr_matrix_project_box(
        matrix.as_mut_ptr(),
        &viewbox,
        transform,
        0f32,
        (*rdata.output).transform_matrix.as_ptr(),
    );
    wl::wlr_render_texture_with_matrix(rdata.renderer, texture, matrix.as_mut_ptr(), 1.0);
    wl::wlr_surface_send_frame_done(surface, rdata.when);
}
unsafe fn render(output: &mut Output) {
    let server = server_ptr();
    let renderer = (*server).renderer;

    let mut now = MaybeUninit::uninit();
    wl::clock_gettime(wl::CLOCK_MONOTONIC as i32, now.as_mut_ptr());
    let now = now.assume_init();

    let mut width = 0;
    let mut height = 0;
    wl::wlr_output_effective_resolution(output.wlr_output, &mut width, &mut height);
    wl::wlr_renderer_begin(renderer, width as u32, height as u32);

    // TODO: Actually take the damage into account and don't just draw all the things. It's silly.
    // TODO: Don't draw all nodes in all outputs. It's silly
    // TODO: the rest

    let color = [0.3, 0.3, 0.3, 1.0];
    wl::wlr_renderer_clear(renderer, color.as_ptr());

    for node in (*server).nodes.values().filter(|x| x.mapped) {
        let mut rdata = RenderData {
            server: &*server,
            output: output.wlr_output,
            node: &**node,
            renderer,
            when: &now,
        };
        wl::wlr_xdg_surface_for_each_surface(
            node.xdg_surface,
            Some(render_surface),
            &mut rdata as *mut RenderData as *mut c_void,
        );
    }

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
    if let Some((_, surface, p)) = find_node(server, cursor_pos(server)) {
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
        if let Some((node, surface, _)) = find_node(server, cursor_pos(server)) {
            let node_id = node.id;
            let toplevel = node.xdg_surface;
            unsafe {
                focus(server, node_id, toplevel, surface);
            }
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
    node_id: NodeId,
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
    wl::wlr_xdg_toplevel_set_activated(toplevel, true);
    wl::wlr_seat_keyboard_notify_enter(
        server.seat,
        (*server.nodes.get(&node_id).unwrap().xdg_surface).surface, // TODO cleanup
        keyboard.keycodes.as_mut_ptr(),
        keyboard.num_keycodes,
        &mut keyboard.modifiers,
    );

    remove_id(&mut server.mru_node, node_id);
    server.mru_node.push(node_id);
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
    let mut caps = wl::wl_seat_capability_WL_SEAT_CAPABILITY_POINTER;
    caps |= wl::wl_seat_capability_WL_SEAT_CAPABILITY_KEYBOARD;

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

fn find_node(
    server: &Server,
    pos: Point,
) -> Option<(&Pin<Box<Node>>, *mut wl::wlr_surface, Point)> {
    // TODO should do in mru order
    server.nodes.values().find_map(|node| {
        let sx = pos.x - node.x as f64;
        let sy = pos.y - node.y as f64;
        let mut sub = Point { x: 0.0, y: 0.0 };
        let surface = unsafe {
            wl::wlr_xdg_surface_surface_at(node.xdg_surface, sx, sy, &mut sub.x, &mut sub.y)
        };
        if surface.is_null() {
            None
        } else {
            Some((node, surface, sub))
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

macro_rules! node {
    ($listener:ident, $field:tt) => {
        &mut *container_of!(Node, $field, $listener)
    };
}

unsafe extern "C" fn xdg_surface_map(listener: *mut wl::wl_listener, _: *mut c_void) {
    let node = node!(listener, map);
    node.mapped = true;
    focus(
        &mut *server_ptr(),
        node.id,
        node.xdg_surface,
        (*node.xdg_surface).surface,
    );
    // TODO: should damage view
}
unsafe extern "C" fn xdg_surface_unmap(listener: *mut wl::wl_listener, _: *mut c_void) {
    let node = node!(listener, unmap);
    node.mapped = false;
    // TODO: should damage view
}
unsafe extern "C" fn xdg_surface_commit(_: *mut wl::wl_listener, _: *mut c_void) {
    invalidate_everything(&*server_ptr());
}
unsafe extern "C" fn xdg_surface_destroy(listener: *mut wl::wl_listener, _: *mut c_void) {
    let id = node!(listener, destroy).id;
    let server = &mut *server_ptr();
    server.nodes.remove(&id);
    remove_id(&mut server.mru_node, id);
}
fn remove_id(ids: &mut Vec<NodeId>, id: NodeId) {
    let mru_idx = ids
        .iter()
        .rposition(|x| *x == id)
        .expect("BUG: node missing from mru vec");
    ids.remove(mru_idx);
}

unsafe extern "C" fn xdg_surface_request_move(listener: *mut wl::wl_listener, _: *mut c_void) {
    let id = node!(listener, request_move).id;

    println!("node {} requested move but we don't care", id);
}
unsafe extern "C" fn xdg_surface_request_resize(listener: *mut wl::wl_listener, _: *mut c_void) {
    let id = node!(listener, request_resize).id;
    println!("node {} requested resize but we don't care", id);
}
