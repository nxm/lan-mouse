use std::{
    fs::File,
    io::{BufWriter, Write},
    net::UdpSocket,
    os::unix::prelude::AsRawFd,
};

use wayland_protocols::{
    wp::{
        pointer_constraints::zv1::client::{zwp_locked_pointer_v1, zwp_pointer_constraints_v1},
        relative_pointer::zv1::client::{zwp_relative_pointer_manager_v1, zwp_relative_pointer_v1},
    },
    xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
};

use wayland_client::{
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, QueueHandle, WEnum,
};

use tempfile;

struct App {
    running: bool,
    compositor: Option<wl_compositor::WlCompositor>,
    buffer: Option<wl_buffer::WlBuffer>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    surface: Option<wl_surface::WlSurface>,
    top_level: Option<xdg_toplevel::XdgToplevel>,
    xdg_surface: Option<xdg_surface::XdgSurface>,
    socket: UdpSocket,
    surface_coords: (f64, f64),
    pointer_constraints: Option<zwp_pointer_constraints_v1::ZwpPointerConstraintsV1>,
    rel_pointer_manager: Option<zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1>,
    pointer_lock: Option<zwp_locked_pointer_v1::ZwpLockedPointerV1>,
    rel_pointer: Option<zwp_relative_pointer_v1::ZwpRelativePointerV1>,
}

fn main() {
    // establish connection via environment-provided configuration.
    let conn = Connection::connect_to_env().unwrap();

    // Retrieve the wayland display object
    let display = conn.display();

    // Create an event queue for our event processing
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    // Create a wl_registry object by sending the wl_display.get_registry request
    display.get_registry(&qh, ());

    let mut app = App {
        running: true,
        compositor: None,
        buffer: None,
        wm_base: None,
        surface: None,
        xdg_surface: None,
        top_level: None,
        socket: UdpSocket::bind("0.0.0.0:42070").expect("couldn't bind to address"),
        surface_coords: (0.0, 0.0),
        pointer_constraints: None,
        rel_pointer_manager: None,
        pointer_lock: None,
        rel_pointer: None,
    };

    // use roundtrip to process this event synchronously
    event_queue.roundtrip(&mut app).unwrap();

    //
    let compositor = app.compositor.as_ref().unwrap();
    app.surface = Some(compositor.create_surface(&qh, ()));
    let wm_base = app.wm_base.as_ref().unwrap();
    app.xdg_surface = Some(wm_base.get_xdg_surface(&app.surface.as_mut().unwrap(), &qh, ()));
    app.top_level = Some(app.xdg_surface.as_ref().unwrap().get_toplevel(&qh, ()));
    app.top_level
        .as_ref()
        .unwrap()
        .set_title("LAN Mouse".into());
    app.surface.as_ref().unwrap().commit();

    while app.running {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

fn draw(f: &mut File, (width, height): (u32, u32)) {
    let mut buf = BufWriter::new(f);
    for y in 0..height {
        for x in 0..width {
            let color: u32 = if (x + y / 8 * 8) % 16 < 8 {
                0xFF8EC07C
            } else {
                0xFFFbF1C7
            };
            buf.write_all(&color.to_ne_bytes()).unwrap();
        }
    }
}

impl App {
    fn send_motion_event(&self, time: u32, x: f64, y: f64) {
        let time_bytes = time.to_ne_bytes();
        let x_bytes = x.to_ne_bytes();
        let y_bytes = y.to_ne_bytes();
        let mut buf: [u8; 20] = [0u8; 20];
        buf[0..4].copy_from_slice(&time_bytes);
        buf[4..12].copy_from_slice(&x_bytes);
        buf[12..20].copy_from_slice(&y_bytes);
        self.socket.send_to(&buf, "192.168.178.114:42069").unwrap();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        app: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<App>,
    ) {
        // Match global event to get globals after requesting them in main
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            // println!("[{}] {} (v{})", name, interface, version);
            match &interface[..] {
                "wl_compositor" => {
                    app.compositor =
                        Some(registry.bind::<wl_compositor::WlCompositor, _, _>(name, 4, qh, ()));
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ());
                    let (width, height) = (64, 64);
                    let mut file = tempfile::tempfile().unwrap();
                    draw(&mut file, (width, height));
                    let pool =
                        shm.create_pool(file.as_raw_fd(), (width * height * 4) as i32, &qh, ());
                    let buffer = pool.create_buffer(
                        0,
                        width as i32,
                        height as i32,
                        (width * 4) as i32,
                        wl_shm::Format::Argb8888,
                        qh,
                        (),
                    );
                    app.buffer = Some(buffer);
                }
                "wl_seat" => {
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
                }
                "xdg_wm_base" => {
                    app.wm_base =
                        Some(registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, &qh, ()));
                }
                "zwp_pointer_constraints_v1" => {
                    app.pointer_constraints = Some(
                        registry.bind::<zwp_pointer_constraints_v1::ZwpPointerConstraintsV1, _, _>(
                            name,
                            1,
                            &qh,
                            (),
                        ),
                    );
                }
                "zwp_relative_pointer_manager_v1" => {
                    app.rel_pointer_manager = Some(
                        registry.bind::<zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1, _, _>(
                            name,
                            1,
                            &qh,
                            (),
                        ),
                    );
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: <wl_compositor::WlCompositor as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        todo!()
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: <wl_surface::WlSurface as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        todo!()
    }
}

impl Dispatch<wl_shm::WlShm, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: <wl_shm::WlShm as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // ignore
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: <wl_shm_pool::WlShmPool as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        todo!()
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: <wl_buffer::WlBuffer as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        //
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: <xdg_wm_base::XdgWmBase as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_wm_base::Event::Ping { serial } => {
                proxy.pong(serial);
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        app: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: <xdg_surface::XdgSurface as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_surface::Event::Configure { serial } => {
                xdg_surface.ack_configure(serial);
                let surface = app.surface.as_ref().unwrap();
                if let Some(ref buffer) = app.buffer {
                    surface.attach(Some(buffer), 0, 0);
                    surface.commit();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(
        app: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: <xdg_toplevel::XdgToplevel as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close {} = event {
            app.running = false;
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        _: &mut Self,
        seat: &wl_seat::WlSeat,
        event: <wl_seat::WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        app: &mut Self,
        pointer: &wl_pointer::WlPointer,
        event: <wl_pointer::WlPointer as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial: _,
                surface: _,
                surface_x,
                surface_y,
            } => {
                app.surface_coords = (surface_x, surface_y);
                if app.pointer_lock.is_none() {
                    app.pointer_lock = Some(app.pointer_constraints.as_ref().unwrap().lock_pointer(
                        &app.surface.as_ref().unwrap(),
                        pointer,
                        None,
                        zwp_pointer_constraints_v1::Lifetime::Persistent,
                        qh,
                        (),
                    ));
                }
                if app.rel_pointer.is_none() {
                    app.rel_pointer = Some(app.rel_pointer_manager
                        .as_ref()
                        .unwrap()
                        .get_relative_pointer(pointer, qh, ()));
                }
            }
            _ => (),
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key { key, .. } = event {
            if key == 1 {
                // ESC key
                if let Some(pointer_lock) = state.pointer_lock.as_ref() {
                    pointer_lock.destroy();
                    state.pointer_lock = None;
                }
                if let Some(rel_pointer) = state.rel_pointer.as_ref() {
                    rel_pointer.destroy();
                    state.rel_pointer = None;
                }
            }
        }
    }
}

impl Dispatch<zwp_pointer_constraints_v1::ZwpPointerConstraintsV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zwp_pointer_constraints_v1::ZwpPointerConstraintsV1,
        _: <zwp_pointer_constraints_v1::ZwpPointerConstraintsV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        //
    }
}

impl Dispatch<zwp_locked_pointer_v1::ZwpLockedPointerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zwp_locked_pointer_v1::ZwpLockedPointerV1,
        event: <zwp_locked_pointer_v1::ZwpLockedPointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_locked_pointer_v1::Event::Locked => {}
            _ => {}
        }
    }
}

impl Dispatch<zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
        _: <zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        //
    }
}

impl Dispatch<zwp_relative_pointer_v1::ZwpRelativePointerV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &zwp_relative_pointer_v1::ZwpRelativePointerV1,
        event: <zwp_relative_pointer_v1::ZwpRelativePointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion {
            utime_hi,
            utime_lo,
            dx: _,
            dy: _,
            dx_unaccel,
            dy_unaccel,
        } = event {
            let time = ((utime_hi as u64) << 32 | utime_lo as u64) / 1000;
            app.send_motion_event(time as u32, dx_unaccel, dy_unaccel);
        }
    }
}
