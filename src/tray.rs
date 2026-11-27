use std::{ptr::{null, null_mut}, sync::{Arc, OnceLock}};
use crate::AppState;
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW, ShellExecuteW},
        WindowsAndMessaging::{
            AppendMenuW, ChangeWindowMessageFilterEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
            DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, MF_STRING, MSG, MSGFLT_ALLOW,
            MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO,
            PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SW_SHOWNORMAL,
            SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_DESTROY,
            WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
        },
    },
};
use windows_sys::w;

const TRAY_EVENT: u32 = WM_APP + 1;
pub enum Action { Restart, Close }
static URL: OnceLock<String> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
static ACTIONS: OnceLock<tokio::sync::mpsc::UnboundedSender<Action>> = OnceLock::new();
static STATE: OnceLock<Arc<AppState>> = OnceLock::new();

pub fn open_browser(url: &str) {
    let address: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    unsafe { ShellExecuteW(null_mut(), w!("open"), address.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

pub fn start(url: String, state: Arc<AppState>) -> tokio::sync::mpsc::UnboundedReceiver<Action> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let _ = ACTIONS.set(sender);
    let _ = STATE.set(state);
    std::thread::spawn(move || {
        let _ = URL.set(url);
        if !run() { eprintln!("MyMeta could not create its notification-area window."); }
    });
    receiver
}

fn run() -> bool {
    let class_name = w!("MyMeta.NotificationArea");
    // Explorer broadcasts this to top-level windows when it recreates the taskbar.
    let _ = TASKBAR_CREATED.set(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance, lpszClassName: class_name, ..Default::default() };
        if RegisterClassW(&class) == 0 { return false; }
        let window = CreateWindowExW(0, class_name, w!("MyMeta"), 0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null());
        if window.is_null() { return false; }
        ChangeWindowMessageFilterEx(window, TRAY_EVENT, MSGFLT_ALLOW, null_mut());
        if let Some(&message) = TASKBAR_CREATED.get().filter(|&&message| message != 0) {
            ChangeWindowMessageFilterEx(window, message, MSGFLT_ALLOW, null_mut());
        }
        add_icon(window, instance);
        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&message);
        }
    }
    true
}

fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: window, uID: 1, ..Default::default() }
}

fn add_icon(window: HWND, instance: HINSTANCE) {
    let mut data = icon_data(window);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = TRAY_EVENT;
    data.hIcon = unsafe { LoadIconW(instance, 1usize as *const u16) };
    for (slot, value) in data.szTip.iter_mut().zip("MyMeta".encode_utf16()) { *slot = value; }
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
        eprintln!("MyMeta could not register its notification-area icon.");
    }
}

fn open_page() {
    if let Some(url) = URL.get() { open_browser(url); }
}

fn show_menu(window: HWND) {
    let command = unsafe {
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor) == 0 { return; }
        let menu = CreatePopupMenu();
        if menu.is_null() { return; }
        AppendMenuW(menu, MF_STRING, 1, w!("打开页面"));
        AppendMenuW(menu, MF_STRING, 2, w!("重启服务"));
        AppendMenuW(menu, MF_STRING, 3, w!("关闭"));
        SetForegroundWindow(window);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, cursor.x, cursor.y, 0, window, null());
        DestroyMenu(menu);
        // The foreground owner needs a posted message to dismiss subsequent tray menus correctly.
        PostMessageW(window, WM_NULL, 0, 0);
        command
    };
    if command == 1 { open_page(); return; }
    let action = match command { 2 => Action::Restart, 3 => Action::Close, _ => return };
    if STATE.get().is_some_and(|state| state.sync_lock.try_lock().is_err() || state.sample_lock.try_write().is_err()) {
        let message = match action {
            Action::Restart => w!("有更新表或读取示例任务正在进行。确定要重启服务吗？"),
            Action::Close => w!("有更新表或读取示例任务正在进行。确定要关闭服务吗？"),
        };
        if unsafe { MessageBoxW(window, message, w!("MyMeta"), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) } != IDYES { return; }
    }
    if let Some(sender) = ACTIONS.get() {
        unsafe { DestroyWindow(window); }
        let _ = sender.send(action);
    }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        TRAY_EVENT => {
            match lparam as u32 {
                WM_LBUTTONDBLCLK => open_page(),
                WM_RBUTTONUP => show_menu(window),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            unsafe { Shell_NotifyIconW(NIM_DELETE, &icon_data(window)); PostQuitMessage(0); }
            0
        }
        message if TASKBAR_CREATED.get() == Some(&message) && message != 0 => {
            add_icon(window, unsafe { GetModuleHandleW(null()) });
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
