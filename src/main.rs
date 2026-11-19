mod limits;

use chrono::{DateTime, Local};
use dispatch2::DispatchQueue;
use limits::{Limits, Window};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, rc::Retained, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSCellImagePosition, NSImage, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusBarButton, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSString};
use std::{
    cell::OnceCell,
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread,
    time::Duration,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

thread_local! {
    static MENU: OnceCell<Menu> = const { OnceCell::new() };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SyncSender<()>]
    struct RefreshTarget;

    impl RefreshTarget {
        #[unsafe(method(refresh:))]
        fn refresh(&self, _sender: &NSMenuItem) {
            let _ = self.ivars().try_send(());
        }
    }
);

struct LimitRows {
    label: Retained<NSMenuItem>,
    reset: Retained<NSMenuItem>,
}

impl LimitRows {
    fn new(menu: &NSMenu, name: &str, mtm: MainThreadMarker) -> Self {
        let label = info_item(menu, name, mtm);
        let reset = info_item(menu, "—", mtm);
        reset.setIndentationLevel(1);
        Self { label, reset }
    }

    fn update(&self, name: &str, window: Option<&Window>) {
        let Some(window) = window else {
            self.label
                .setTitle(&NSString::from_str(&format!("{name} · Unavailable")));
            self.reset.setHidden(true);
            return;
        };
        self.label.setTitle(&NSString::from_str(&format!(
            "{name} · {} left",
            remaining_percent(window)
        )));
        let reset = window
            .resets_at
            .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
            .map(|date| {
                format!(
                    "Resets {}",
                    date.with_timezone(&Local).format("%a %d %b, %H:%M")
                )
            })
            .unwrap_or_else(|| "Reset time unavailable".into());
        self.reset.setTitle(&NSString::from_str(&reset));
        self.reset.setHidden(false);
    }
}

fn remaining_percent(window: &Window) -> String {
    let remaining = (100.0 - window.used_percent).clamp(0.0, 100.0);
    format!("{remaining:.0}%")
}

struct Menu {
    _status_item: Retained<NSStatusItem>,
    button: Option<Retained<NSStatusBarButton>>,
    _refresh_target: Retained<RefreshTarget>,
    five_hour: LimitRows,
    weekly: LimitRows,
    updated: Retained<NSMenuItem>,
    error: Retained<NSMenuItem>,
}

impl Menu {
    fn new(app: &NSApplication, requests: SyncSender<()>, mtm: MainThreadMarker) -> Self {
        let status_item =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let button = status_item.button(mtm);
        if let Some(button) = &button {
            let title = NSString::from_str("Codex Usage");
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("gauge.with.dots.needle.50percent"),
                Some(&title),
            );
            if let Some(image) = image {
                image.setTemplate(true);
                button.setImage(Some(&image));
                button.setImagePosition(NSCellImagePosition::ImageLeading);
            }
            button.setTitle(&NSString::from_str("Codex…"));
            button.setToolTip(Some(&title));
        }

        let menu = NSMenu::new(mtm);
        let five_hour = LimitRows::new(&menu, "5-hour limit · Loading…", mtm);
        let weekly = LimitRows::new(&menu, "Weekly limit · Loading…", mtm);
        five_hour.reset.setHidden(true);
        weekly.reset.setHidden(true);
        let updated = info_item(&menu, "", mtm);
        updated.setHidden(true);
        let error = info_item(&menu, "", mtm);
        error.setHidden(true);
        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let refresh_target: Retained<RefreshTarget> =
            unsafe { objc2::msg_send![super(RefreshTarget::alloc(mtm).set_ivars(requests)), init] };
        let refresh = NSMenuItem::new(mtm);
        refresh.setTitle(&NSString::from_str("Refresh"));
        unsafe {
            refresh.setAction(Some(sel!(refresh:)));
            refresh.setTarget(Some(&refresh_target));
        }
        menu.addItem(&refresh);

        let quit = NSMenuItem::new(mtm);
        quit.setTitle(&NSString::from_str("Quit Codex Menu Bar"));
        quit.setKeyEquivalent(&NSString::from_str("q"));
        unsafe {
            quit.setAction(Some(sel!(terminate:)));
            quit.setTarget(Some(app));
        }
        menu.addItem(&quit);
        status_item.setMenu(Some(&menu));

        Self {
            _status_item: status_item,
            button,
            _refresh_target: refresh_target,
            five_hour,
            weekly,
            updated,
            error,
        }
    }

    fn update(&self, result: Result<Limits, String>) {
        match result {
            Ok(limits) => {
                self.set_summary(&limits);
                self.five_hour
                    .update("5-hour limit", limits.five_hour.as_ref());
                self.weekly.update("Weekly limit", limits.weekly.as_ref());
                self.updated.setTitle(&NSString::from_str(&format!(
                    "Updated at {}",
                    Local::now().format("%H:%M")
                )));
                self.updated.setHidden(false);
                self.error.setHidden(true);
            }
            Err(error) => {
                eprintln!("Codex Menu Bar: {error}");
                if self.updated.isHidden() {
                    self.set_summary(&Limits::default());
                    self.five_hour.update("5-hour limit", None);
                    self.weekly.update("Weekly limit", None);
                }
                self.error.setTitle(&NSString::from_str(&error));
                self.error.setHidden(false);
            }
        }
    }

    fn set_summary(&self, limits: &Limits) {
        if let Some(button) = &self.button {
            let five_hour = limits
                .five_hour
                .as_ref()
                .map_or_else(|| "—".into(), remaining_percent);
            let weekly = limits
                .weekly
                .as_ref()
                .map_or_else(|| "—".into(), remaining_percent);
            button.setTitle(&NSString::from_str(&format!(
                "5h {five_hour} · 7d {weekly}"
            )));
        }
    }
}

fn info_item(menu: &NSMenu, title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setEnabled(false);
    menu.addItem(&item);
    item
}

fn refresh_loop(requests: Receiver<()>) {
    loop {
        let result = limits::fetch();
        DispatchQueue::main().exec_async(move || {
            MENU.with(|menu| {
                if let Some(menu) = menu.get() {
                    menu.update(result);
                }
            });
        });
        match requests.recv_timeout(REFRESH_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("AppKit requires the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let (sender, requests) = mpsc::sync_channel(1);
    MENU.with(|menu| {
        menu.get_or_init(|| Menu::new(&app, sender, mtm));
    });
    thread::spawn(move || refresh_loop(requests));
    app.run();
}
