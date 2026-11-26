//! 正在运行的壁纸对外的统一接口。
//!
//! 命令行（`muse next` 等）、托盘、设计窗口、`muse mcp` 都通过这里控制壁纸、改设置。
//! 通道：Linux、macOS 用本地 Unix socket，Windows 用命名管道；同时用它保证只有一个实例在运行。
//!
//! 协议：每行一条 JSON。
//! - 请求 `{"id": 1, "method": "next", "params": {...}}`
//! - 回复 `{"id": 1, "result": ...}` 或 `{"id": 1, "error": {"message": "..."}}`
//! - 发过 `subscribe` 的连接还会收到事件 `{"event": "status" | "changed", "data": ...}`
//!
//! 方法见 [`crate::api`]。

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// 发给前端事件循环的命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Next,
    Prev,
    /// 切到第几首。
    Goto(usize),
    TogglePause,
    SetPaused(bool),
    Quit,
    /// 重新读取配置、位置和样式。
    Reload,
}

/// 发命令后唤醒前端的事件循环。
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// 可以克隆给托盘、接口线程的发送端。
#[derive(Clone)]
pub struct Remote {
    tx: Sender<Command>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Remote {
    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
        if let Some(w) = self
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            w();
        }
    }
}

/// 前端持有的接收端。
pub struct Inbox {
    rx: Receiver<Command>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Inbox {
    /// 前端的事件循环建好后登记唤醒方式。
    pub fn set_waker(&self, w: Waker) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
    }

    pub fn try_recv(&self) -> Option<Command> {
        self.rx.try_recv().ok()
    }
}

pub fn channel_pair() -> (Remote, Inbox) {
    let (tx, rx) = channel();
    let waker = Arc::new(Mutex::new(None));
    (
        Remote {
            tx,
            waker: waker.clone(),
        },
        Inbox { rx, waker },
    )
}

/// 壁纸当前的状态，前端更新，接口线程读。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub paused: bool,
    pub index: usize,
}

/// 接口线程和前端共享的东西。
pub struct Hub {
    pub remote: Remote,
    status: Mutex<Status>,
    /// 订阅了事件的连接。
    subscribers: Mutex<Vec<Sender<String>>>,
    /// 改设置、改样式的历史，用来撤销；同一时间只有一个写操作。
    pub store: Mutex<crate::api::Store>,
    pub ctx: crate::api::Context,
}

impl Hub {
    pub fn new(remote: Remote, ctx: crate::api::Context) -> Arc<Self> {
        Arc::new(Self {
            remote,
            status: Mutex::default(),
            subscribers: Mutex::default(),
            store: Mutex::default(),
            ctx,
        })
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 前端换了诗或暂停状态时调用；有变化才通知订阅者。
    pub fn set_status(&self, index: usize, paused: bool) {
        let s = Status { paused, index };
        let mut cur = self.status.lock().unwrap_or_else(|e| e.into_inner());
        if *cur == s {
            return;
        }
        *cur = s;
        drop(cur);
        self.broadcast("status", crate::api::status_json(self));
    }

    /// 给所有订阅者发一个事件；断开的连接顺便清掉。
    pub fn broadcast(&self, event: &str, data: Value) {
        let line = json!({ "event": event, "data": data }).to_string();
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|tx| tx.send(line.clone()).is_ok());
    }

    fn subscribe(&self, tx: Sender<String>) {
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
    }
}

/// 一行请求最长多少字节，防止异常客户端把内存吃光。
const MAX_LINE: usize = 16 << 20;

/// 处理一个连接：逐行读请求、回复；订阅后事件也从这个连接发出。
fn serve(reader: impl Read, mut writer: impl Write + Send + 'static, hub: Arc<Hub>) {
    let (tx, rx) = channel::<String>();
    // 回复和事件都经同一个写线程发出，不会交错。
    let writer_thread = std::thread::Builder::new()
        .name("muse-api-write".into())
        .spawn(move || {
            for line in rx {
                if writer
                    .write_all(line.as_bytes())
                    .and_then(|_| writer.write_all(b"\n"))
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
    if writer_thread.is_err() {
        return;
    }
    let mut reader = BufReader::new(reader.take(u64::MAX));
    let mut line = String::new();
    loop {
        line.clear();
        match (&mut reader).take(MAX_LINE as u64).read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if !line.ends_with('\n') && line.len() >= MAX_LINE => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let method = req.get("method").and_then(Value::as_str).unwrap_or("");
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                if method == "subscribe" {
                    hub.subscribe(tx.clone());
                }
                match crate::api::handle(&hub, method, &params) {
                    Ok(result) => json!({ "id": id, "result": result }),
                    Err(e) => json!({ "id": id, "error": { "message": e } }),
                }
            }
            Err(e) => {
                json!({ "id": null, "error": { "message": format!("请求不是有效的 JSON: {e}") } })
            }
        };
        if tx.send(reply.to_string()).is_err() {
            break;
        }
    }
}

/// 客户端：连上正在运行的 muse，发一个请求，等回复。
pub fn call(method: &str, params: Value) -> Result<Value, String> {
    let (reader, mut writer) = ipc::connect()?;
    let req = json!({ "id": 1, "method": method, "params": params });
    writer
        .write_all(format!("{req}\n").as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|e| format!("发送失败: {e}"))?;
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err("muse 关闭了连接".into()),
            Ok(_) => {}
            Err(e) => return Err(format!("读取回复失败: {e}")),
        }
        let v: Value =
            serde_json::from_str(&line).map_err(|e| format!("回复不是有效的 JSON: {e}"))?;
        if v.get("event").is_some() {
            continue;
        }
        if let Some(e) = v.get("error") {
            return Err(e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_string());
        }
        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// 没有在运行时给的提示。
pub const NOT_RUNNING: &str = "muse 没有在运行；先运行 muse run（或从开机自启、托盘启动）";

#[cfg(unix)]
mod ipc {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::{Hub, NOT_RUNNING};

    pub fn socket_path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        // 没有 XDG_RUNTIME_DIR 时放在公共临时目录，用用户 id 区分。
        let uid = rustix::process::getuid().as_raw();
        dir.join(format!("muse-{uid}.sock"))
    }

    /// 锁文件：有 XDG_RUNTIME_DIR 时和 socket 放一起；退回公共临时目录时放到自己的状态目录，
    /// 免得别的用户预先建好同名文件让 muse 起不来。
    fn lock_path(socket: &std::path::Path) -> PathBuf {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|v| !v.is_empty());
        match muse_core::config::state_dir() {
            Some(dir) if !runtime && std::fs::create_dir_all(&dir).is_ok() => dir.join("muse.lock"),
            _ => socket.with_extension("lock"),
        }
    }

    /// 占住 socket 并在后台线程里接受连接。已有实例在运行时返回错误。
    pub fn listen(hub: Arc<Hub>) -> Result<(), String> {
        let path = socket_path();
        // 先拿文件锁：两个 muse run 同时启动时，后来的那个不会删掉前一个的 socket。
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path(&path))
            .map_err(|e| format!("{}: 创建锁文件失败: {e}", path.display()))?;
        if rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err()
            || UnixStream::connect(&path).is_ok()
        {
            return Err("muse 已经在运行；用 muse quit 退出它，或 muse next 切换下一首".into());
        }
        // 锁一直持有到进程退出。
        std::mem::forget(lock);
        // 上次异常退出留下的 socket 文件。
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)
            .map_err(|e| format!("{}: 创建控制 socket 失败: {e}", path.display()))?;
        // 放在公共临时目录时也只允许自己连。
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        std::thread::Builder::new()
            .name("muse-api".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let Ok(writer) = stream.try_clone() else {
                        continue;
                    };
                    let hub = hub.clone();
                    let _ = std::thread::Builder::new()
                        .name("muse-api-conn".into())
                        .spawn(move || super::serve(stream, writer, hub));
                }
            })
            .map_err(|e| format!("启动控制线程失败: {e}"))?;
        Ok(())
    }

    /// 退出时删掉 socket 文件。
    pub fn cleanup() {
        let _ = std::fs::remove_file(socket_path());
    }

    pub fn connect() -> Result<(UnixStream, UnixStream), String> {
        let s = UnixStream::connect(socket_path()).map_err(|_| NOT_RUNNING.to_string())?;
        let w = s.try_clone().map_err(|e| format!("连接失败: {e}"))?;
        Ok((s, w))
    }

    pub fn address() -> String {
        socket_path().to_string_lossy().into_owned()
    }
}

#[cfg(windows)]
mod ipc {
    //! 命名管道。服务端用重叠 I/O：同一个连接上读线程阻塞在读时，写线程照样能写
    //! （同步句柄上的读写会互相排队，订阅事件的连接会卡死）。

    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::sync::Arc;

    use windows_sys::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile,
        WriteFile,
    };
    use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW};

    use super::{Hub, NOT_RUNNING};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// 命名管道的名字，按用户区分。
    pub fn pipe_name() -> String {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
        let user: String = user
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        format!(r"\\.\pipe\muse-{user}")
    }

    /// 一次重叠操作用的事件。
    struct Event(OwnedHandle);

    impl Event {
        fn new() -> std::io::Result<Self> {
            // 安全性：参数都为空表示默认安全属性、手动重置、初始无信号、不命名。
            let h = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if h.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            // 安全性：h 是刚创建的有效句柄，所有权交给 OwnedHandle。
            Ok(Self(unsafe { OwnedHandle::from_raw_handle(h as _) }))
        }
    }

    /// 等一次重叠操作完成，返回传输的字节数。
    fn finish(pipe: HANDLE, ok: i32, ov: &OVERLAPPED) -> std::io::Result<u32> {
        if ok == 0 {
            // 安全性：只读取线程的最后错误码。
            let err = unsafe { GetLastError() };
            if err != ERROR_IO_PENDING {
                return Err(std::io::Error::from_raw_os_error(err as i32));
            }
        }
        let mut n = 0u32;
        // 安全性：ov 在操作完成前一直有效（调用方在这里等到完成才返回）；最后一个参数为真表示等待。
        if unsafe { GetOverlappedResult(pipe, ov, &mut n, 1) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(n)
    }

    /// 服务端的一端：读、写各用自己的事件，可以在两个线程里同时进行。
    struct Pipe {
        handle: Arc<OwnedHandle>,
        event: Event,
    }

    impl Pipe {
        fn raw(&self) -> HANDLE {
            self.handle.as_raw_handle() as HANDLE
        }

        fn overlapped(&self) -> OVERLAPPED {
            OVERLAPPED {
                hEvent: self.event.0.as_raw_handle() as HANDLE,
                ..Default::default()
            }
        }
    }

    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut ov = self.overlapped();
            let len = buf.len().min(u32::MAX as usize) as u32;
            // 安全性：buf 和 ov 在 finish 等到操作完成之前都有效。
            let ok = unsafe {
                ReadFile(
                    self.raw(),
                    buf.as_mut_ptr(),
                    len,
                    std::ptr::null_mut(),
                    &mut ov,
                )
            };
            match finish(self.raw(), ok, &ov) {
                Ok(n) => Ok(n as usize),
                Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
                Err(e) => Err(e),
            }
        }
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut ov = self.overlapped();
            let len = buf.len().min(u32::MAX as usize) as u32;
            // 安全性：buf 和 ov 在 finish 等到操作完成之前都有效。
            let ok =
                unsafe { WriteFile(self.raw(), buf.as_ptr(), len, std::ptr::null_mut(), &mut ov) };
            finish(self.raw(), ok, &ov).map(|n| n as usize)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 只允许管道的创建者（本用户）和系统访问；默认 DACL 会给所有人读权限。
    struct Security(PSECURITY_DESCRIPTOR);

    impl Security {
        fn new() -> Option<Self> {
            let sddl = wide("D:P(A;;GA;;;OW)(A;;GA;;;SY)");
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // 安全性：sddl 以 0 结尾；成功时 sd 由系统分配，Drop 里用 LocalFree 释放。
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then_some(Self(sd))
        }
    }

    // 安全性：安全描述符创建后只读，由这个值独占，跨线程使用没有数据竞争。
    unsafe impl Send for Security {}

    impl Drop for Security {
        fn drop(&mut self) {
            // 安全性：self.0 是 ConvertStringSecurityDescriptorToSecurityDescriptorW 分配的。
            unsafe { LocalFree(self.0 as _) };
        }
    }

    fn create_pipe(
        name: &[u16],
        first: bool,
        sec: Option<&Security>,
    ) -> Result<OwnedHandle, String> {
        let attrs = sec.map(|s| SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: s.0,
            bInheritHandle: 0,
        });
        let flags = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // 安全性：name 以 0 结尾；attrs 在调用期间有效（为空时用默认安全属性）。
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                attrs.as_ref().map_or(std::ptr::null(), |a| a as *const _),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            // 安全性：只读取线程的最后错误码。
            return Err(format!("创建命名管道失败（错误 {}）", unsafe {
                GetLastError()
            }));
        }
        // 安全性：h 是刚创建的有效句柄，所有权交给 OwnedHandle。
        Ok(unsafe { OwnedHandle::from_raw_handle(h as _) })
    }

    /// 等客户端连上这个管道实例。
    fn accept(h: &OwnedHandle) -> std::io::Result<()> {
        let event = Event::new()?;
        let mut ov = OVERLAPPED {
            hEvent: event.0.as_raw_handle() as HANDLE,
            ..Default::default()
        };
        let raw = h.as_raw_handle() as HANDLE;
        // 安全性：句柄有效；ov 在 finish 等到连接完成之前有效。
        let ok = unsafe { ConnectNamedPipe(raw, &mut ov) };
        if ok == 0 && unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
            return Ok(());
        }
        finish(raw, ok, &ov).map(|_| ())
    }

    /// 用命名 mutex 保证单实例（句柄留到进程退出），再在后台线程里接受管道连接。
    pub fn listen(hub: Arc<Hub>) -> Result<(), String> {
        let name = wide("Local\\muse-wallpaper");
        // 安全性：name 是以 0 结尾的 UTF-16 字符串。
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if !handle.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err("muse 已经在运行；请用托盘菜单或 muse quit 退出它".into());
        }
        let pipe = wide(&pipe_name());
        let sec = Security::new();
        // 第一个实例同步创建：名字被别人占了就直接报错。
        let mut next = create_pipe(&pipe, true, sec.as_ref())?;
        std::thread::Builder::new()
            .name("muse-api".into())
            .spawn(move || {
                loop {
                    let conn = next;
                    if let Err(e) = accept(&conn) {
                        eprintln!("命名管道连接失败: {e}");
                    }
                    // 先建好下一个实例再处理这个连接，缩短客户端连不上的空档。
                    next = loop {
                        match create_pipe(&pipe, false, sec.as_ref()) {
                            Ok(h) => break h,
                            Err(e) => {
                                eprintln!("{e}");
                                std::thread::sleep(std::time::Duration::from_secs(1));
                            }
                        }
                    };
                    let handle = Arc::new(conn);
                    let (Ok(r), Ok(w)) = (Event::new(), Event::new()) else {
                        continue;
                    };
                    let reader = Pipe {
                        handle: handle.clone(),
                        event: r,
                    };
                    let writer = Pipe { handle, event: w };
                    let hub = hub.clone();
                    let _ = std::thread::Builder::new()
                        .name("muse-api-conn".into())
                        .spawn(move || super::serve(reader, writer, hub));
                }
            })
            .map_err(|e| format!("启动控制线程失败: {e}"))?;
        Ok(())
    }

    pub fn cleanup() {}

    /// 客户端：先读后写、不并发，用普通的同步句柄就行；管道忙时等一会儿再试。
    pub fn connect() -> Result<(File, File), String> {
        let name = pipe_name();
        let wname = wide(&name);
        for attempt in 0..5 {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
            {
                Ok(f) => {
                    let w = f.try_clone().map_err(|e| format!("连接失败: {e}"))?;
                    return Ok((f, w));
                }
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                    // 安全性：wname 以 0 结尾。
                    unsafe { WaitNamedPipeW(wname.as_ptr(), 2000) };
                }
                // 服务端在两个管道实例之间的空档，稍等再试。
                Err(_) if attempt < 2 => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        Err(NOT_RUNNING.to_string())
    }

    pub fn address() -> String {
        pipe_name()
    }
}

#[cfg(not(any(unix, windows)))]
mod ipc {
    use std::sync::Arc;

    use super::Hub;

    pub fn listen(_: Arc<Hub>) -> Result<(), String> {
        Ok(())
    }

    pub fn cleanup() {}

    pub fn connect() -> Result<(std::io::Empty, std::io::Sink), String> {
        Err("这个平台不支持命令控制".into())
    }

    pub fn address() -> String {
        String::new()
    }
}

pub use ipc::{address, cleanup, listen};
