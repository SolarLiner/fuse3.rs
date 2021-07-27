use std::borrow::Borrow;
use std::ffi::{c_void, CStr, CString};
use std::mem::{size_of_val, MaybeUninit};
use std::num::NonZeroU8;
use std::ops::Deref;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use lazy_static::lazy_static;
use nix::errno::Errno;
use structopt::StructOpt;

use fuse3_dev::{raw::*, Fuse, FuseArgs};

#[derive(StructOpt, Debug)]
struct HelloFuse {
    /// Filename to appear as
    #[structopt(short, long)]
    filename: PathBuf,
    /// Contents of the file
    #[structopt(short, long)]
    contents: String,
    /// Filesystem moubtpoint
    #[structopt(name = "mountpoint")]
    mountpoint: PathBuf,
    #[structopt(name = "FUSE args")]
    fuse_args: Vec<String>,
}

lazy_static! {
    static ref FUSE_OPTIONS: HelloFuse = HelloFuse::from_args();
    static ref FUSE_OPER: fuse_operations = fuse_operations {
        init: Some(hello_init),
        getattr: Some(hello_getattr),
        readdir: Some(hello_readdir),
        open: Some(hello_open),
        read: Some(hello_read),
        ..Default::default()
    };
}

unsafe extern "C" fn hello_init(_conn: *mut fuse_conn_info, cfg: *mut fuse_config) -> *mut c_void {
    (*cfg).kernel_cache = 1;
    eprintln!("trace: hello_init");
    return null_mut();
}

unsafe extern "C" fn hello_getattr(
    path: *const c_char,
    stbuf: *mut stat,
    _fi: *mut fuse_file_info,
) -> c_int {
    let path = CStr::from_ptr(path);
    let path = path.to_string_lossy();
    eprintln!("trace: hello_getattr {}", path);
    if path == "/" {
        (*stbuf).st_mode = S_IFDIR | 0755;
        (*stbuf).st_nlink = 2;
        return 0;
    } else if path[1..] == FUSE_OPTIONS.filename.display().to_string() {
        (*stbuf).st_mode = S_IFREG | 0444;
        (*stbuf).st_nlink = 1;
        (*stbuf).st_size = FUSE_OPTIONS.contents.len() as i64;
        return 0;
    }
    return -(Errno::ENOENT as i32);
}

unsafe extern "C" fn hello_readdir(
    path: *const c_char,
    buf: *mut c_void,
    filler: fuse_fill_dir_t,
    _offset: off_t,
    _fi: *mut fuse_file_info,
    _flags: fuse_readdir_flags,
) -> c_int {
    let filler = match filler {
        Some(f) => f,
        None => return -(Errno::EPROTO as i32),
    };
    let path = CStr::from_ptr(path as *mut _);
    let path = path.to_string_lossy();
    eprintln!("trace: hello_readdir {}", path);
    if path != "/" {
        return -(Errno::ENOENT as i32);
    }
    return [
        b"." as &[u8],
        b"..",
        FUSE_OPTIONS.filename.display().to_string().as_bytes(),
    ]
    .iter()
    .map(|b| {
        b.iter()
            .filter_map(|b| NonZeroU8::new(*b))
            .collect::<Vec<_>>()
    })
    .map(|v| CString::from(v))
    .map(|cs| filler(buf, cs.as_ptr(), null_mut(), 0, 0))
    .filter(|r| *r != 0)
    .next()
    .unwrap_or(0);
}

unsafe extern "C" fn hello_open(path: *const c_char, fi: *mut fuse_file_info) -> c_int {
    let path = CStr::from_ptr(path as *mut _);
    let path = path.to_string_lossy();
    eprintln!("trace: hello_open {}", path);

    if path[1..] != FUSE_OPTIONS.filename.display().to_string() {
        return -(Errno::ENOENT as i32);
    }
    if ((*fi).flags & O_ACCMODE as i32) != O_RDONLY as i32 {
        return -(Errno::EACCES as i32);
    }
    return 0;
}

unsafe extern "C" fn hello_read(
    path: *const c_char,
    buf: *mut c_char,
    size: size_t,
    offset: off_t,
    _fi: *mut fuse_file_info,
) -> c_int {
    let path = CStr::from_ptr(path as *mut _);
    let path = path.to_string_lossy();
    eprintln!("trace: hello_read {} (off: {}, size: {})", path, offset, size);

    if path[1..] != FUSE_OPTIONS.filename.display().to_string() {
        return -(Errno::ENOENT as i32);
    }
    let len = FUSE_OPTIONS.contents.len() as off_t;
    if offset < len {
        let size = if offset + size as off_t > len {
            len - offset
        } else {
            size as off_t
        };
        let content_ptr = FUSE_OPTIONS.contents.as_ptr().add(offset as usize);
        eprintln!("hello_read: copying {}b to content_ptr", size);
        std::ptr::copy_nonoverlapping(content_ptr, buf as *mut _, size as usize);
        size as c_int
    } else {
        0
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = FuseArgs::from_iter(
        std::iter::once(std::env::current_exe()?.display().to_string())
            .chain(FUSE_OPTIONS.fuse_args.iter().cloned()),
    )?;
    let mut fuse = Fuse::<()>::new(&args, &*FUSE_OPER, None).unwrap();
    fuse.mount(FUSE_OPTIONS.mountpoint.as_path())?;
    fuse.loop_single()?;
    fuse.unmount();
    fuse.finalize();
    Ok(())
}
