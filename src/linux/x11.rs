#![allow(non_upper_case_globals)]

//! X11 input events.
//!
//! This module uses the XRecord extension to monitor core input events. If the server supports it,
//! it will also monitor XInput2 smooth scroll state.

use crate::{
    ButtonState, Event, EventError, ListenError, ScrollUnit,
    button::convert::x11_button,
    key::convert::{from_keysym, linux_keycode},
};
use byor::fd::{Fd, RuntimeFd};
use derive_more::{Deref, DerefMut};
use futures_core::{FusedStream, Stream};
use std::{
    collections::HashMap,
    ffi::*,
    marker::PhantomData,
    mem::MaybeUninit,
    os::fd::BorrowedFd,
    ptr::null,
    sync::mpsc::{Receiver, Sender, TryRecvError, channel},
    task::Poll,
};
use tracing::{error, instrument};
use x11::{xinput2::*, xlib::*, xrecord::*};
use xkeysym::Keysym;

type Result<T, E = EventError> = std::result::Result<T, E>;

/// Callback closure.
struct Closure {
    xi2: bool,
    display: *mut Display,
    tx: Sender<Result<Event>>,
}

pub fn events<Rt: RuntimeFd>() -> Result<impl FusedStream<Item = Result<Event>>, ListenError> {
    let control_display = unsafe { XOpenDisplay(null()) };
    // This is the display that XRecord hijacks in order to listen to events. It is used with
    // EnableContext and ProcessReplies. Its file descriptor is used to poll for events.
    let data_display = unsafe { XOpenDisplay(null()) };
    if data_display.is_null() || control_display.is_null() {
        return Err(ListenError::Platform);
    }

    let mut range = unsafe { XRecordAllocRange() };
    if range.is_null() {
        return Err(ListenError::Platform);
    }
    unsafe {
        (*range).device_events.first = KeyPress as c_uchar;
        (*range).device_events.last = MotionNotify as c_uchar;
    }
    let mut clients = XRecordAllClients;

    let context = unsafe {
        XRecordCreateContext(
            control_display,
            /*datum_flags:*/ 0,
            &raw mut clients,
            /*n_clients:*/ 1,
            &raw mut range,
            /*n_ranges:*/ 1,
        )
    };

    // Range has been copied.
    unsafe { XFree(range as *mut _) };

    if context == 0 {
        return Err(ListenError::Platform);
    }

    // The context creation has to be flushed before it can be enabled.
    unsafe { XSync(control_display, False) };

    let mut xi_opcode = MaybeUninit::uninit();
    let xi2 = unsafe {
        XQueryExtension(
            control_display,
            c"XInputExtension".as_ptr(),
            xi_opcode.as_mut_ptr(),
            MaybeUninit::uninit().as_mut_ptr(),
            MaybeUninit::uninit().as_mut_ptr(),
        )
    } == True
        && {
            let mut major = 2;
            let mut minor = 3;

            match unsafe { XIQueryVersion(control_display, &raw mut major, &raw mut minor) }
                as c_uchar
            {
                Success => true,
                BadRequest => false,
                _ => return Err(ListenError::Platform),
            }
        };

    let (tx, rx) = channel();
    let closure = Box::into_raw(Box::new(Closure {
        xi2,
        display: control_display,
        tx,
    }));

    if unsafe {
        XRecordEnableContextAsync(
            data_display,
            context,
            Some(xrecord_callback),
            closure.cast(),
        )
    } == 0
    {
        return Err(ListenError::Platform);
    }

    // X11 sockets aren't non-blocking. However, all I do is poll, and that doesn't require
    // non-blocking mode.
    let record_fd =
        Rt::Fd::new(unsafe { BorrowedFd::borrow_raw(XConnectionNumber(data_display)) })?;

    const fn max(a: i32, b: i32) -> i32 {
        if a > b { a } else { b }
    }

    let xi = if xi2 {
        let window = unsafe { XDefaultRootWindow(control_display) };
        let mut mask = [0; (max(XI_RawMotion, XI_DeviceChanged) as usize).div_ceil(8)];
        XISetMask(&mut mask, XI_RawMotion);
        XISetMask(&mut mask, XI_DeviceChanged);

        let mut event_mask = XIEventMask {
            deviceid: XIAllMasterDevices,
            mask_len: mask.len() as c_int,
            mask: mask.as_mut_ptr(),
        };
        if unsafe { XISelectEvents(control_display, window, &raw mut event_mask, 1) }
            != Success as c_int
        {
            return Err(ListenError::Platform);
        }

        Some(XIInfo {
            fd: Rt::Fd::new(unsafe { BorrowedFd::borrow_raw(XConnectionNumber(control_display)) })?,
            opcode: unsafe { xi_opcode.assume_init() },
            scroll_info: HashMap::default(),
        })
    } else {
        None
    };

    Ok(X11Stream {
        rx,
        record_fd,
        xi,
        data_display,
        closure,
        context,
        process: true,
        dc: false,
    })
}

struct X11Stream<F> {
    rx: Receiver<Result<Event>>,
    record_fd: F,
    xi: Option<XIInfo<F>>,
    data_display: *mut Display,
    // Can't hold a Box because it would violate aliasing rules.
    closure: *mut Closure,
    context: c_ulong,
    process: bool,
    dc: bool,
}

struct XIInfo<F> {
    fd: F,
    opcode: c_int,
    scroll_info: HashMap<c_int, (XIScrollClassInfo, XIScrollClassInfo)>,
}

impl<F> Drop for X11Stream<F> {
    fn drop(&mut self) {
        let control_display = unsafe { (*self.closure).display };
        unsafe {
            XRecordDisableContext(control_display, self.context);
            XRecordFreeContext(control_display, self.context);

            // Context disable has to be flushed in order to avoid use-after-free.
            XSync(control_display, False);
            drop(Box::from_raw(self.closure));

            XCloseDisplay(self.data_display);
            XCloseDisplay(control_display);
        }
    }
}

#[derive(Deref, DerefMut)]
#[deref(forward)]
struct XIDeviceInfoBox(&'static mut XIDeviceInfo);

impl Drop for XIDeviceInfoBox {
    fn drop(&mut self) {
        unsafe { XIFreeDeviceInfo(self.0 as *mut _) };
    }
}

struct XEventDataBox<T: 'static> {
    cookie: XGenericEventCookie,
    _marker: PhantomData<&'static mut T>,
    display: *mut Display,
}

impl<T: 'static> XEventDataBox<T> {
    unsafe fn new(display: *mut Display, cookie: XGenericEventCookie) -> Self {
        Self {
            cookie,
            _marker: PhantomData,
            display,
        }
    }
}

impl<T: 'static> std::ops::Deref for XEventDataBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.cookie.data as *const _) }
    }
}

impl<T: 'static> std::ops::DerefMut for XEventDataBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *(self.cookie.data as *mut _) }
    }
}

impl<T> Drop for XEventDataBox<T> {
    fn drop(&mut self) {
        unsafe { XFreeEventData(self.display, &raw mut self.cookie) };
    }
}

/// <div class="warning">This function may block. Make sure to check that there is pending data with
/// <code>XPending</code> before calling.</div>
fn smooth_scroll(
    display: *mut Display,
    opcode: c_int,
    scroll_info: &mut HashMap<c_int, (XIScrollClassInfo, XIScrollClassInfo)>,
) -> Option<Result<Event>> {
    let mut event = MaybeUninit::uninit();
    unsafe { XNextEvent(display, event.as_mut_ptr()) };
    let mut event = unsafe { event.assume_init() };

    if unsafe { event.type_ } != GenericEvent
        || unsafe { &event.generic_event_cookie }.extension != opcode
    {
        return None;
    }
    if unsafe { XGetEventData(display, &raw mut event.generic_event_cookie) } != True {
        return Some(Err(EventError::GetState));
    }
    match unsafe { &event.generic_event_cookie }.evtype {
        XI_RawMotion => {}
        XI_DeviceChanged => {
            let event = unsafe {
                XEventDataBox::<XIDeviceChangedEvent>::new(display, event.generic_event_cookie)
            };
            scroll_info.remove(&event.sourceid);
        }
        _ => return None,
    }

    let event = unsafe { XEventDataBox::<XIRawEvent>::new(display, event.generic_event_cookie) };
    let mut n_devices = 1;
    let device_info = XIDeviceInfoBox(unsafe {
        &mut *XIQueryDevice(display, event.sourceid, &raw mut n_devices)
    });
    assert!(
        n_devices == 1,
        "XIQueryDevice returned more than one device"
    );

    let (vertical, horizontal) = match scroll_info.get(&event.sourceid) {
        Some(info) => Some(info),
        None => {
            let mut vertical = None;
            let mut horizontal = None;
            for class in unsafe {
                std::slice::from_raw_parts(device_info.classes, device_info.num_classes as usize)
            } {
                if unsafe { (**class)._type } == XIScrollClass {
                    if vertical.is_some() && horizontal.is_some() {
                        break;
                    }
                    let info = unsafe { *(*class as *mut XIScrollClassInfo) };
                    if info.scroll_type == XIScrollTypeVertical {
                        vertical = Some(info);
                    } else if info.scroll_type == XIScrollTypeHorizontal {
                        horizontal = Some(info);
                    }
                }
            }
            scroll_info.insert(event.sourceid, (vertical?, horizontal?));
            scroll_info.get(&event.sourceid)
        }
    }?;

    let mask = unsafe {
        std::slice::from_raw_parts(event.valuators.mask, event.valuators.mask_len as usize)
    };
    let n_valuators = mask.iter().map(|x| x.count_ones() as usize).sum();
    let valuators = unsafe { std::slice::from_raw_parts(event.valuators.values, n_valuators) };

    fn byte_level_mask(index: usize) -> u8 {
        1 << (index % 8)
    }

    let valuator = |index| {
        if let Some(byte) = mask.get(index / 8)
            && (byte & byte_level_mask(index)) != 0
        {
            let pos = (0..index)
                .filter(|i| (byte & byte_level_mask(*i)) != 0)
                .count();
            valuators.get(pos).copied()
        } else {
            None
        }
    };

    Some(Ok(Event::Scroll {
        unit: ScrollUnit::Detent,
        dx: valuator(horizontal.number as usize)
            .map(|v| v / horizontal.increment)
            .unwrap_or_default(),
        dy: valuator(vertical.number as usize)
            .map(|v| v / vertical.increment)
            .unwrap_or_default(),
    }))
}

impl<F: Fd> Stream for X11Stream<F> {
    type Item = Result<Event>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.dc {
            return Poll::Ready(None);
        }
        let control_display = unsafe { (*self.closure).display };
        let this = unsafe { self.get_unchecked_mut() };

        'out: loop {
            if this.process {
                unsafe { XRecordProcessReplies(this.data_display) };
                if let Some(XIInfo {
                    opcode,
                    scroll_info,
                    ..
                }) = &mut this.xi
                {
                    while unsafe { XPending(control_display) } > 0 {
                        if let Some(r) = smooth_scroll(control_display, *opcode, scroll_info) {
                            break 'out Poll::Ready(Some(r));
                        }
                    }
                }
                this.process = false;
            }
            match this.rx.try_recv() {
                Ok(e) => break Poll::Ready(Some(e)),
                Err(TryRecvError::Disconnected) => {
                    this.dc = true;
                    break Poll::Ready(None);
                }
                Err(TryRecvError::Empty) => match this.record_fd.poll_readable(cx) {
                    Poll::Ready(Ok(())) => {
                        this.process = true;
                    }
                    Poll::Ready(Err(e)) => break Poll::Ready(Some(Err(e.into()))),
                    Poll::Pending => {
                        if let Some(XIInfo { fd, .. }) = &this.xi {
                            match fd.poll_readable(cx) {
                                Poll::Ready(Ok(())) => {
                                    this.process = true;
                                }
                                v => {
                                    break v.map(|r| {
                                        Some(r.map(|_| unreachable!()).map_err(Into::into))
                                    });
                                }
                            }
                        } else {
                            break Poll::Pending;
                        };
                    }
                },
            }
        }
    }
}

impl<F: Fd> FusedStream for X11Stream<F> {
    fn is_terminated(&self) -> bool {
        self.dc
    }
}

#[derive(Deref, DerefMut)]
#[deref(forward)]
struct XRecordDataBox(&'static mut XRecordInterceptData);

impl Drop for XRecordDataBox {
    fn drop(&mut self) {
        unsafe { XRecordFreeData(self.0 as *mut _) };
    }
}

#[instrument(skip_all)]
unsafe extern "C" fn xrecord_callback(closure: *mut c_char, data: *mut XRecordInterceptData) {
    let Closure { xi2, display, tx } = unsafe { &mut *(closure as *mut _) };
    let data = XRecordDataBox(unsafe { &mut *data });
    if data.category != XRecordFromServer {
        return;
    }

    let bytes =
        unsafe { std::slice::from_raw_parts(data.data as *const _, data.data_len as usize * 4) };

    let category = bytes[0] as c_int;
    let raw_keycode = bytes[1];

    match category {
        KeyPress | KeyRelease => {
            let keycode = linux_keycode(raw_keycode as u16 - 8);

            let mut state = MaybeUninit::uninit();
            if unsafe {
                XkbGetState(*display, 0x100 /*XkbUseCoreKbd*/, state.as_mut_ptr())
            } != Success as c_int
            {
                if let Err(error) = tx.send(Err(EventError::GetState)) {
                    error!(%error, "Failed to send error");
                }
                return;
            }

            let keysym = unsafe {
                XkbKeycodeToKeysym(
                    *display,
                    raw_keycode,
                    state.assume_init().group as c_int,
                    /*level:*/ 1,
                )
            };
            let key = from_keysym(Keysym::new(keysym as u32));

            let state = if category == KeyPress {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };

            if let Err(error) = tx.send(Ok(Event::Key {
                key,
                keycode,
                state,
            })) {
                error!(%error, "Failed to send event");
            }
        }
        MotionNotify => {
            let mut root_x = u16::from_ne_bytes(bytes[20..22].try_into().unwrap());
            let mut root_y = u16::from_ne_bytes(bytes[22..24].try_into().unwrap());
            if data.client_swapped == True {
                root_x = root_x.swap_bytes();
                root_y = root_y.swap_bytes();
            }

            if let Err(error) = tx.send(Ok(Event::Motion {
                x: root_x as i32,
                y: root_y as i32,
            })) {
                error!(%error, "Failed to send event");
            }
        }
        ButtonPress | ButtonRelease => {
            let event = match bytes[1] {
                4..8 if *xi2 => return,
                4 => Event::Scroll {
                    unit: ScrollUnit::Detent,
                    dx: 0.0,
                    dy: -1.0,
                },
                5 => Event::Scroll {
                    unit: ScrollUnit::Detent,
                    dx: 0.0,
                    dy: 1.0,
                },
                6 => Event::Scroll {
                    unit: ScrollUnit::Detent,
                    dx: -1.0,
                    dy: 0.0,
                },
                7 => Event::Scroll {
                    unit: ScrollUnit::Detent,
                    dx: 1.0,
                    dy: 0.0,
                },
                code => Event::Button {
                    button: x11_button(code),
                    state: if category == ButtonPress {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                },
            };
            if let Err(error) = tx.send(Ok(event)) {
                error!(%error, "Failed to send event");
            }
        }
        _ => {}
    }
}
