use crate::{
    Event, EventError, ListenError,
    key::convert::{linux_keycode, x11_key},
};
use byor::fd::{Fd, RuntimeFd};
use futures_core::Stream;
use std::{
    ffi::{c_char, c_int, c_uchar, c_uint, c_ulong},
    marker::PhantomData,
    mem::MaybeUninit,
    os::fd::{AsFd, AsRawFd, BorrowedFd},
    ptr::null,
    sync::mpsc::{Receiver, Sender, TryRecvError, channel},
    task::Poll,
};
use tracing::{error, instrument};
use x11::{xlib::*, xrecord::*};

type Result<T, E = EventError> = std::result::Result<T, E>;

/// Callback closure.
struct Closure {
    display: *mut Display,
    tx: Sender<Result<Event>>,
}

pub fn listen<Rt: RuntimeFd>() -> Result<impl Stream<Item = Result<Event>>, ListenError> {
    let control_display = unsafe { XOpenDisplay(null()) };
    // This is the display that XRecord hijacks in order to listen to events. It is used with
    // EnableContext and ProcessReplies. Its file descriptor is used to poll for events.
    let data_display = unsafe { XOpenDisplay(null()) };

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

    let (tx, rx) = channel();
    let closure = Box::into_raw(Box::new(Closure {
        display: control_display,
        tx,
    }));

    let result = unsafe {
        XRecordEnableContextAsync(
            data_display,
            context,
            Some(xrecord_callback),
            closure.cast(),
        )
    };
    if result == 0 {
        return Err(ListenError::Platform);
    }
    let fd = Rt::Fd::new(unsafe { BorrowedFd::borrow_raw(XConnectionNumber(data_display)) })?;

    Ok(X11Stream {
        rx,
        fd,
        _marker: PhantomData,
        control_display,
        data_display,
        closure,
        context,
        dc: false,
        process: true,
    })
}

struct X11Stream<F, I> {
    rx: Receiver<Result<Event>>,
    fd: F,
    _marker: PhantomData<I>,
    control_display: *mut Display,
    data_display: *mut Display,
    closure: *mut Closure,
    context: c_ulong,
    dc: bool,
    process: bool,
}

impl<F, I> Drop for X11Stream<F, I> {
    fn drop(&mut self) {
        unsafe {
            XRecordDisableContext(self.control_display, self.context);
            XRecordFreeContext(self.control_display, self.context);

            // Context disable has to be flushed in order to avoid use-after-free.
            XSync(self.control_display, False);
            drop(Box::from_raw(self.closure));

            XCloseDisplay(self.data_display);
            XCloseDisplay(self.control_display);
        }
    }
}

impl<T: AsFd + AsRawFd, F: Fd<T>> Stream for X11Stream<F, T> {
    type Item = Result<Event>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.dc {
            return Poll::Ready(None);
        }

        let this = unsafe { self.get_unchecked_mut() };

        loop {
            if this.process {
                unsafe { XRecordProcessReplies(this.data_display) };
                this.process = false;
            }
            match this.rx.try_recv() {
                Ok(e) => break Poll::Ready(Some(e)),
                Err(TryRecvError::Disconnected) => {
                    this.dc = true;
                    break Poll::Ready(None);
                }
                Err(TryRecvError::Empty) => match this.fd.poll_readable(cx) {
                    Poll::Ready(Ok(())) => {
                        this.process = true;
                    }
                    v => break v.map(|r| Some(r.map(|_| unreachable!()).map_err(Into::into))),
                },
            }
        }
    }
}

struct XRecordInterceptDataBox(&'static mut XRecordInterceptData);

impl std::ops::Deref for XRecordInterceptDataBox {
    type Target = XRecordInterceptData;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl std::ops::DerefMut for XRecordInterceptDataBox {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl Drop for XRecordInterceptDataBox {
    fn drop(&mut self) {
        unsafe { XRecordFreeData(self.0 as *mut _) };
    }
}

#[instrument(skip_all)]
unsafe extern "C" fn xrecord_callback(closure: *mut c_char, data: *mut XRecordInterceptData) {
    let Closure { display, tx } = unsafe { &mut *(closure as *mut _) };
    let data = XRecordInterceptDataBox(unsafe { &mut *data });
    if data.category != XRecordFromServer {
        return;
    }

    let bytes =
        unsafe { std::slice::from_raw_parts(data.data as *const _, data.data_len as usize * 4) };

    let category = bytes[0] as c_int;
    let raw_keycode = bytes[1];

    #[allow(non_upper_case_globals)]
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
            let key = x11_key(keysym as c_uint);

            let e = if category == KeyPress {
                Event::KeyPress { key, keycode }
            } else {
                Event::KeyRelease { key, keycode }
            };

            if let Err(error) = tx.send(Ok(e)) {
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
        _ => {}
    }
}
