mod icon;
mod limits;

use chrono::{DateTime, Local};
use dispatch2::{DispatchQueue, DispatchTime};
use icon::{Animation, Levels};
use limits::{Limits, Window};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, rc::Retained, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSCellImagePosition, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusBarButton, NSStatusItem, NSVariableStatusItemLength, NSWorkspace,
};
use objc2_foundation::{NSObject, NSString};
use std::{
    cell::{Cell, OnceCell},
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
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
            "{name} · {:.0}% left",
            window.remaining()
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

struct Menu {
    _status_item: Retained<NSStatusItem>,
    button: Option<Retained<NSStatusBarButton>>,
    icon_levels: Cell<Levels>,
    animation_generation: Cell<u64>,
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
            button.setImagePosition(NSCellImagePosition::ImageOnly);
            button.setImage(Some(&icon::image([None, None])));
            button.setToolTip(Some(&NSString::from_str(
                "Codex Usage · 5-hour and weekly limits",
            )));
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
            icon_levels: Cell::new([None, None]),
            animation_generation: Cell::new(0),
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
                self.update_icon([
                    limits
                        .five_hour
                        .as_ref()
                        .map(|window| window.remaining() / 100.0),
                    limits
                        .weekly
                        .as_ref()
                        .map(|window| window.remaining() / 100.0),
                ]);
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
                    self.five_hour.update("5-hour limit", None);
                    self.weekly.update("Weekly limit", None);
                }
                self.error.setTitle(&NSString::from_str(&error));
                self.error.setHidden(false);
            }
        }
    }

    fn draw_icon(&self, levels: Levels) {
        if let Some(button) = &self.button {
            button.setImage(Some(&icon::image(levels)));
            self.icon_levels.set(levels);
        }
    }

    fn update_icon(&self, target: Levels) {
        let generation = self.animation_generation.get().wrapping_add(1);
        self.animation_generation.set(generation);
        if self.button.is_none()
            || NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
        {
            self.draw_icon(target);
            return;
        }
        self.animation_frame(Animation::new(generation, self.icon_levels.get(), target));
    }

    fn animation_frame(&self, animation: Animation) {
        if animation.generation != self.animation_generation.get() {
            return;
        }
        let (levels, finished) = animation.frame(Instant::now());
        self.draw_icon(levels);
        if finished {
            return;
        }
        let next_frame =
            DispatchTime::try_from(icon::FRAME_INTERVAL).expect("valid frame interval");
        if DispatchQueue::main()
            .after(next_frame, move || {
                MENU.with(|menu| {
                    if let Some(menu) = menu.get() {
                        menu.animation_frame(animation);
                    }
                });
            })
            .is_err()
        {
            self.draw_icon(animation.target);
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
