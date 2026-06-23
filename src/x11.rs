use crate::geometry::Point;
use anyhow::anyhow;
use x11::xlib;

pub fn assert_x11_available() -> anyhow::Result<()> {
    unsafe {
        let display = xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err(anyhow!("could not connect to X11 DISPLAY"));
        }
        xlib::XCloseDisplay(display);
    }
    Ok(())
}

pub fn pointer_position() -> anyhow::Result<Point> {
    unsafe {
        let display = xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err(anyhow!("could not connect to X11 DISPLAY"));
        }
        let root = xlib::XDefaultRootWindow(display);
        let mut root_return = 0;
        let mut child_return = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask_return = 0;
        let ok = xlib::XQueryPointer(
            display,
            root,
            &mut root_return,
            &mut child_return,
            &mut root_x,
            &mut root_y,
            &mut win_x,
            &mut win_y,
            &mut mask_return,
        );
        xlib::XCloseDisplay(display);
        if ok == 0 {
            return Err(anyhow!("could not query X11 pointer position"));
        }
        Ok(Point::new(root_x, root_y))
    }
}
