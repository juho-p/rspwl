// unsafe {
//     lets goooo

use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::ptr;
use std::ffi::c_void;

use wl_sys as wl;

use crate::{window_manager::{WindowManager, WindowRef}, wlroots_compositor::server::*};

use super::{server::View, wl_util::*};
use crate::types::NodeId;

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

    let renderer = wl::wlr_renderer_autocreate(backend);
    if !wl::wlr_renderer_init_wl_display(renderer, wl_display) {
        panic!("Failed to initialize display");
    }

    let allocator = wl::wlr_allocator_autocreate(backend, renderer);

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
        allocator,

        xdg_shell,

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

        wm: WindowManager::new(),
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

    set_server(&mut server);

    let socket = wl::wl_display_add_socket_auto(server.wl_display);
    if socket.is_null() {
        wl::wlr_backend_destroy(server.backend);
    } else {
        if !wl::wlr_backend_start(server.backend) {
            wl::wlr_backend_destroy(server.backend);
            wl::wl_display_destroy(server.wl_display);
        } else {
            println!("Run display");
            wl::wl_display_run(server.wl_display);

            wl::wl_display_destroy_clients(server.wl_display);
            wl::wl_display_destroy(server.wl_display);
        }
    }

    set_server(ptr::null_mut());
}

// ---

fn new_output(server: &mut Server, wlr_output: &mut wl::wlr_output, _: ()) {
    unsafe {
        wl::wlr_output_init_render(wlr_output, server.allocator, server.renderer);

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

    server.add_output(output);
}

fn new_xdg_surface(server: &mut Server, xdg_surface: &mut wl::wlr_xdg_surface, _: ()) {
    if xdg_surface.role != wl::wlr_xdg_surface_role_WLR_XDG_SURFACE_ROLE_TOPLEVEL {
        println!("xdg-shell popup (this log is just noise)");
        return;
    }

    server.wm.add_view(|id| {
        unsafe { View::from_xdg_toplevel_surface(id, xdg_surface) }
    });

    // TODO remove this
    invalidate_everything(server);
}

unsafe extern "C" fn damage_handle_frame(listener: *mut wl::wl_listener, _: *mut c_void) {
    let output = &mut *container_of!(Output, damage_frame, listener);

    if !(*output.wlr_output).enabled {
        println!("Output disabled");
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
    } = output_coords(server.output_layout, output);
    ox += (view.rect.x + sx as f32) as f64;
    oy += (view.rect.y + sy as f32) as f64;

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
            println!("- RECT {:?}", scissor_box);

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

    println!("BEGIN RENDER");
    for window in server.wm.views_for_render(output.id) {
        match &window.view.shell_surface {
            ShellView::Empty => println!("Empty view (WHY???)"),
            ShellView::Xdg(xdgview) => {
                xdg_surface_for_each_surface(xdgview.xdgsurface.xdg_surface, |s, x, y| {
                    println!("-surface");
                    render_surface(s, x, y, server, output, &window.view, &now, damage);
                })
            }
        }
        // wl::wlr_xdg_surface_for_each_surface(
        //     view.xdg_surface,
        //     Some(render_surface),
        //     &mut rdata as *mut RenderData as *mut c_void,
        // );
    }
    wl::wlr_renderer_scissor(server.renderer, ptr::null_mut());
    wl::wlr_output_render_software_cursors(output.wlr_output, ptr::null_mut());
    println!("END RENDER");

    wl::wlr_renderer_end(renderer);
}

fn output_destroy(server: &mut Server, _: &mut (), id: OutputId) {
    // TODO test if it fine that this destroys the damage as well
    server.remove_output(id);
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
    if let Some((_, surface, p)) = find_window(server, cursor_pos(server)) {
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

        wl::wlr_seat_pointer_clear_focus(server.seat);
    }
}

fn cursor_button(server: &mut Server, event: &mut wl::wlr_event_pointer_button, _: ()) {
    unsafe {
        wl::wlr_seat_pointer_notify_button(server.seat, event.time_msec, event.button, event.state);
    }

    if event.state == wl::wlr_button_state_WLR_BUTTON_RELEASED {
        // TODO release button here (resize drag, etc)
    } else {
        let to_focus = if let Some((window, surface, _)) = find_window(server, cursor_pos(server)) {
            let focus_toplevel = match &window.view.shell_surface {
                ShellView::Xdg(v) => Some(v.xdgsurface.xdg_surface),
                ShellView::Empty => None,
                // NOTE: doesn't really work for non xdg, but that's a problem for another day
            };
            println!("clicked view {}", window.view.id);
            let view_id = window.view.id;

            focus_toplevel.map(|x| (view_id, surface, x))
        } else {
            println!("clicked outside view");
            None
        };

        if let Some((view_id, surface, xdgsurface)) = to_focus {
            unsafe {
                focus_xdg(server, xdgsurface, view_id, surface);
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

unsafe fn focus_xdg(
    server: &mut Server,
    xdg_surface: *mut wl::wlr_xdg_surface,
    view_id: NodeId,
    surface: *mut wl::wlr_surface,
) {
    // We have mut ref to server and ref to view. breaks borrow checker. hope Rust won't mind
    println!("set focus");
    let prev_surface = (*server.seat).keyboard_state.focused_surface;
    if prev_surface == surface {
        return;
    }
    if !prev_surface.is_null() {
        let prev = wl::wlr_xdg_surface_from_wlr_surface(prev_surface);
        if !prev.is_null() {
            wl::wlr_xdg_toplevel_set_activated(prev, false);
        }
    }

    let keyboard = &mut *wl::wlr_seat_get_keyboard(server.seat);

    wl::wlr_xdg_toplevel_set_activated(xdg_surface, true);
    wl::wlr_seat_keyboard_notify_enter(
        server.seat,
        surface,
        keyboard.keycodes.as_mut_ptr(),
        keyboard.num_keycodes,
        &mut keyboard.modifiers,
    );

    server.wm.touch_node(view_id);
}

fn handle_new_input(server: &mut Server, device: &mut wl::wlr_input_device, _: ()) {
    // New input device available

    if device.type_ == wl::wlr_input_device_type_WLR_INPUT_DEVICE_KEYBOARD {
        let id = (1..255).find(|i| server.keyboards.iter().all(|x| x.id != *i));
        if let Some(id) = id {
            // TODO keyboard destroy not handled??
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

                let context = wl::xkb_context_new(wl::xkb_context_flags_XKB_CONTEXT_NO_FLAGS);
                let keymap = wl::xkb_keymap_new_from_names(
                    context,
                    ptr::null(),
                    wl::xkb_keymap_compile_flags_XKB_KEYMAP_COMPILE_NO_FLAGS,
                );

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

            println!("Add keyboard");
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
            println!("Focused client requests cursor");
            wl::wlr_cursor_set_surface(
                server.cursor,
                event.surface,
                event.hotspot_x,
                event.hotspot_y,
            );
        } else {
            println!("Unfocused client TRIED to request cursor");
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

// TODO move?
fn find_window<'a>(
    server: &'a Server,
    pos: Point,
) -> Option<(
    WindowRef,
    *mut wl::wlr_surface,
    Point,
)> {
    let mut views = server.wm.views_for_finding();

    views.find_map(|content| match &content.view.shell_surface {
        ShellView::Xdg(xdgview) => {
            let xdg_surface = xdgview.xdgsurface.xdg_surface;
            let sx = pos.x - content.view.rect.x as f64;
            let sy = pos.y - content.view.rect.y as f64;
            let mut sub = Point { x: 0.0, y: 0.0 };
            let surface = unsafe {
                wl::wlr_xdg_surface_surface_at(xdg_surface, sx, sy, &mut sub.x, &mut sub.y)
            };
            if surface.is_null() {
                None
            } else {
                Some((content, surface, sub))
            }
        }
        ShellView::Empty => None,
    })
}

fn invalidate_everything(server: &Server) {
    for output in server.outputs.iter() {
        unsafe {
            wl::wlr_output_damage_add_whole(output.damage);
        }
    }
}
