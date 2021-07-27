use std::borrow::{Borrow, BorrowMut};
use std::ffi::{c_void, CStr, CString};
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::mem::size_of_val;
use std::ops::{Deref, DerefMut};
use std::os::raw::{c_char, c_int, c_long};
use std::path::Path;
use std::pin::Pin;
use std::ptr::{null, null_mut};

pub use nix;
use nix::errno::Errno;
use nix::libc;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
pub mod raw {
    pub mod low {
        include!(concat!(env!("OUT_DIR"), "/bindings_low.rs"));
    }
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub struct FFIRef<'a, T: ?Sized> {
    __phantom: PhantomData<&'a T>,
    _value: *mut T,
}

impl<'a, T: ?Sized> Deref for FFIRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self._value }
    }
}

impl<'a, T: ?Sized> DerefMut for FFIRef<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self._value }
    }
}

pub struct FFIBox<T: ?Sized> {
    value: *mut T,
    run_free: bool,
    destructor: unsafe extern "C" fn(*mut T),
}

impl<T: ?Sized> FFIBox<T> {
    /// Create a owned box with the given destructor.
    ///
    /// Safety: Caller must ensure that the value is valid, as it will be treated as a mutable reference.
    /// The destructor must deinitialize the value's memory.
    #[inline]
    pub unsafe fn create(
        value: *mut T,
        run_free: bool,
        destructor: unsafe extern "C" fn(*mut T),
    ) -> Self {
        Self {
            value,
            destructor,
            run_free,
        }
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> *const T {
        self.value
    }

    #[inline(always)]
    pub fn as_mut(&mut self) -> *mut T {
        self.value
    }

    #[inline(always)]
    pub fn transfer_ownership(self) -> *mut T {
        let v = self.value;
        std::mem::forget(self);
        v
    }

    pub fn borrow(&self) -> FFIRef<T> {
        FFIRef {
            __phantom: PhantomData,
            _value: self.value,
        }
    }
}

impl<T> FFIBox<T> {
    /// Creates an owned value on the heap with the contents of `val` and the provided destructor.
    pub fn new(val: T, destructor: unsafe extern "C" fn(*mut T)) -> Self {
        let value = unsafe { libc::malloc(size_of_val(&val)) } as *mut T;
        unsafe {
            value.write(val);
        }
        Self {
            value,
            destructor,
            run_free: true,
        }
    }
}

impl<T: ?Sized> Drop for FFIBox<T> {
    fn drop(&mut self) {
        unsafe { (self.destructor)(self.value) }
        if self.run_free {
            unsafe { libc::free(self.value as *mut c_void) }
        }
    }
}

impl<T: ?Sized> Deref for FFIBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.value }
    }
}

impl<T: ?Sized> DerefMut for FFIBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.value }
    }
}

impl<T: ?Sized> AsRef<T> for FFIBox<T> {
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<T: ?Sized> AsMut<T> for FFIBox<T> {
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}

impl<T: ?Sized> Borrow<T> for FFIBox<T> {
    fn borrow(&self) -> &T {
        self.deref()
    }
}

impl<T: ?Sized> BorrowMut<T> for FFIBox<T> {
    fn borrow_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}

impl<T: Debug> Debug for FFIBox<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "FFIBox(")?;
        Debug::fmt(self.as_ref(), f)?;
        write!(f, ")")
    }
}

impl<T: Display> Display for FFIBox<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.as_ref(), f)
    }
}

pub struct FuseArgs {
    _value: FFIBox<raw::fuse_args>,
}

impl Deref for FuseArgs {
    type Target = raw::fuse_args;
    fn deref(&self) -> &Self::Target {
        self._value.as_ref()
    }
}

impl FuseArgs {
    /// Parse FUSE args from the given String iterator.
    ///
    /// TODO: Make it configurable
    pub fn from_iter(iter: impl Iterator<Item = String>) -> nix::Result<Self> {
        let argv = iter
            .map(|s| CString::new(s).unwrap().into_boxed_c_str())
            .map(|s| Box::leak(s).as_ptr() as *mut _)
            .collect::<Vec<_>>()
            .leak();

        let argc = argv.len() as c_int;
        let mut fuse_args = raw::fuse_args {
            argc,
            argv: argv.as_mut_ptr(),
            allocated: 0,
        };
        Errno::result(unsafe { raw::fuse_opt_parse(&mut fuse_args, null_mut(), null(), None) })?;
        Ok(Self {
            _value: FFIBox::new(fuse_args, raw::fuse_opt_free_args),
        })
    }

    /// Parse FUSE args from command-line arguments
    pub fn from_args() -> nix::Result<Self> {
        Self::from_iter(std::env::args())
    }

    /// Create FUSE args from a mount-path.
    pub fn from_mountpath(path: &Path) -> nix::Result<Self> {
        Self::from_iter(
            vec![
                std::env::current_exe()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                path.to_string_lossy().to_string(),
            ]
            .into_iter(),
        )
    }

    /// Shows FUSE lib help, querying requested modules for their options to show help to,
    pub fn show_help(&self) {
        unsafe { raw::fuse_lib_help(self._value.as_ptr() as *mut _) }
    }
}

impl Debug for FuseArgs {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let data = (0..self.argc)
            .map(|i| {
                unsafe { self.argv.add(i as usize).as_ref() }.map(|&p| {
                    unsafe { CStr::from_ptr(p as *const _) }
                        .to_string_lossy()
                        .to_string()
                })
            })
            .collect::<Vec<_>>();
        let alloc = self.allocated == 1;
        f.debug_struct("FuseArgs")
            .field("allocated", &alloc)
            .field("args", &data)
            .finish()
    }
}

/// Raw FUSE driver.
pub struct Fuse<T: ?Sized> {
    _value: FFIBox<raw::fuse>,
    private_data: Option<Box<T>>,
}

impl<T: ?Sized> Fuse<T> {
    pub fn finalize(mut self) {
        unsafe { raw::fuse_exit(self._value.as_mut()) }
    }
}

impl<T: ?Sized> Fuse<T> {
    pub fn from_ops(ops: &raw::fuse_operations, private_data: impl Into<Option<Box<T>>>) -> Self {
        let mut private_data = private_data.into();
        let r = unsafe {
            raw::fuse_new(
                null_mut(),
                ops,
                size_of_val(ops) as _,
                private_data
                    .as_mut()
                    .map(|p| p.as_mut() as *mut _ as *mut c_void)
                    .unwrap_or(null_mut()),
            )
        };
        unsafe { r.as_mut() }
            .map(|p| Self {
                _value: unsafe { FFIBox::create(p, false, raw::fuse_destroy) },
                private_data,
            })
            // Raw pointer should always be valid as a null arg input is considered empty
            .expect("FUSE unknown error")
    }
}

impl<T: ?Sized> Deref for Fuse<T> {
    type Target = raw::fuse;
    fn deref(&self) -> &Self::Target {
        self._value.as_ref()
    }
}

impl<T: ?Sized> Fuse<T> {
    /// Returns None if an unknown argument is passed to `args`.
    pub fn new(
        args: &FuseArgs,
        operations: &raw::fuse_operations,
        private_data: impl Into<Option<Box<T>>>,
    ) -> Option<Self> {
        let mut private_data = private_data.into();
        let r = unsafe {
            raw::fuse_new(
                args._value.as_ptr() as *mut _,
                operations as *const _ as *mut _,
                size_of_val(operations) as _,
                private_data
                    .as_mut()
                    .map(|p| p as *mut _ as *mut c_void)
                    .unwrap_or(null_mut()),
            )
        };
        unsafe { r.as_mut() }.map(|value| Self {
            // Safety: The pointer returned by `fuse_new`, is non-null, is always valid.
            _value: unsafe { FFIBox::create(value, false, raw::fuse_destroy) },
            private_data,
        })
    }

    pub fn mount(&mut self, mount: &Path) -> Result<(), Errno> {
        let mount = CString::new(mount.display().to_string()).unwrap();
        Errno::result(unsafe { raw::fuse_mount(self._value.as_mut(), mount.as_ptr() as *mut _) })
            .map(|_| ())
    }

    pub fn unmount(&mut self) {
        unsafe { raw::fuse_unmount(self._value.as_mut()) }
    }

    pub fn loop_single(&self) -> Result<(), Errno> {
        let r = unsafe { raw::fuse_loop(self._value.as_ptr() as *mut _) };
        if r < 0 {
            Err(Errno::from_i32(-r))
        } else {
            Ok(())
        }
    }
}
