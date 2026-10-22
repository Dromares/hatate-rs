//! Window assembly and the headless driver (shots, measurements, audits, Tab order).

use crate::audit::{self, Pixels, Rect};
use crate::fixtures;
use crate::fonts;
use crate::measure::{self, count_over, summarize};
use crate::options::{Open, Options};
use crate::queue::{self, Queue, Status};
use crate::review::{self, Review};
use crate::theme::{self, Scheme};
use crate::thumbs::ThumbCache;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, graphene};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

pub const APP_ID: &str = "org.hatate.GtkSpike";

/// Set by any audit line that says FAIL; the process then exits 1, so CI can gate on it.
static AUDIT_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `PASS` or `FAIL`, remembering a failure for the exit code.
fn verdict(ok: bool) -> &'static str {
    if !ok {
        AUDIT_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if ok { "PASS" } else { "FAIL" }
}

/// Everything the driver needs to reach after the window is built.
#[derive(Clone)]
struct Ui {
    app: adw::Application,
    win: adw::ApplicationWindow,
    stack: gtk4::Stack,
    queue: Queue,
    review: Rc<Review>,
    dropdown: gtk4::DropDown,
    menubar: gtk4::PopoverMenuBar,
    tabs: Vec<gtk4::ToggleButton>,
    thumbs: Option<ThumbCache>,
}

fn page(title: &str, body: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    page.add_css_class("page");
    let t = gtk4::Label::new(Some(title));
    t.add_css_class("title");
    t.set_xalign(0.0);
    page.append(&t);
    page.append(body);
    page
}

fn load_side(path: &Option<std::path::PathBuf>, generated: image::RgbaImage) -> review::Loaded {
    match path {
        Some(p) => review::load(p),
        None => Ok(generated),
    }
}

/// Run the application with `opts`. `start` is the process start, for first-frame timing.
pub fn run(opts: Options, start: Instant) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| build(app, &opts, start));
    let code = app.run_with_args::<&str>(&[]);
    if AUDIT_FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        return glib::ExitCode::from(1);
    }
    code
}

/// The menubar model: File and View. View carries the sort actions and their shortcuts.
fn menu_model() -> gio::Menu {
    let file = gio::Menu::new();
    file.append(Some("Remove selected\u{2026}"), Some("win.remove-selected"));
    file.append(Some("Quit"), Some("app.quit"));
    let sort = gio::Menu::new();
    sort.append(Some("Sort by file"), Some("win.sort::FILE"));
    sort.append(Some("Sort by status"), Some("win.sort::STATUS"));
    sort.append(Some("Sort by matches"), Some("win.sort::MATCHES"));
    let view = gio::Menu::new();
    view.append_section(None, &sort);
    view.append(Some("Descending"), Some("win.sort-descending"));
    let menu = gio::Menu::new();
    menu.append_submenu(Some("File"), &file);
    menu.append_submenu(Some("View"), &view);
    menu
}

/// Actions and shortcuts. Sorting is an action with accelerators, not a Tab stop on the column
/// headers (Oderisi, DAN-1968 section 4). Every activation is logged as an `ACTION` line, so a
/// run driven from a real keyboard shows which shortcut fired.
fn install_actions(app: &adw::Application, win: &adw::ApplicationWindow, queue: &Queue) {
    let col = Rc::new(RefCell::new(String::new()));
    let desc = Rc::new(Cell::new(false));

    let sort =
        gio::SimpleAction::new_stateful("sort", Some(glib::VariantTy::STRING), &"".to_variant());
    {
        let (queue, col, desc) = (queue.clone(), col.clone(), desc.clone());
        sort.connect_activate(move |a, p| {
            let c: String = p.and_then(|v| v.get()).unwrap_or_default();
            a.set_state(&c.to_variant());
            queue.sort_by(&c, desc.get());
            println!("ACTION sort {c} descending={}", desc.get());
            *col.borrow_mut() = c;
        });
    }
    win.add_action(&sort);

    let sort_desc = gio::SimpleAction::new_stateful("sort-descending", None, &false.to_variant());
    {
        let (queue, col, desc) = (queue.clone(), col, desc);
        sort_desc.connect_activate(move |a, _| {
            let now = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            a.set_state(&now.to_variant());
            desc.set(now);
            let c = col.borrow().clone();
            if !c.is_empty() {
                queue.sort_by(&c, now);
            }
            println!("ACTION sort-descending {now} column={c:?}");
        });
    }
    win.add_action(&sort_desc);

    let remove = gio::SimpleAction::new("remove-selected", None);
    {
        let (win, queue) = (win.clone(), queue.clone());
        remove.connect_activate(move |_, _| {
            println!("ACTION remove-selected");
            confirm_dialog(queue.selected_count()).present(Some(&win));
        });
    }
    win.add_action(&remove);

    let quit = gio::SimpleAction::new("quit", None);
    {
        let app = app.clone();
        quit.connect_activate(move |_, _| {
            println!("ACTION quit");
            app.quit();
        });
    }
    app.add_action(&quit);

    app.set_accels_for_action("win.sort::FILE", &["<Ctrl>1"]);
    app.set_accels_for_action("win.sort::STATUS", &["<Ctrl>2"]);
    app.set_accels_for_action("win.sort::MATCHES", &["<Ctrl>3"]);
    app.set_accels_for_action("win.sort-descending", &["<Ctrl><Shift>d"]);
    app.set_accels_for_action("win.remove-selected", &["Delete"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

fn confirm_dialog(selected: u64) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(
        Some("Remove selected rows?"),
        Some(&format!(
            "{selected} rows leave the queue. The image files stay where they are."
        )),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog
}

fn build(app: &adw::Application, opts: &Options, start: Instant) {
    if opts.theme {
        theme::install(opts.scheme);
    } else {
        // Control run: libadwaita as shipped, in the requested scheme.
        adw::StyleManager::default().set_color_scheme(match opts.scheme {
            Scheme::Dark => adw::ColorScheme::ForceDark,
            Scheme::Light => adw::ColorScheme::ForceLight,
        });
    }

    let (gl, gr) = fixtures::sample_pair();
    let review = Rc::new(Review::new(
        load_side(&opts.left, gl),
        load_side(&opts.right, gr),
        opts.mode,
    ));

    let thumbs = opts
        .thumbs
        .as_ref()
        .map(|dir| ThumbCache::new(dir.clone(), opts.thumb_cache, opts.thumb_workers));

    let t_load = Instant::now();
    let store = queue::build_store(opts.rows);
    let queue = Queue::new(&store, thumbs.clone());
    let load_ms = t_load.elapsed().as_secs_f64() * 1e3;
    // Sort first: a selection is a set of positions in the sorted model.
    if let Some((col, desc)) = &opts.sort {
        if !queue.sort_by(col, *desc) {
            eprintln!("unknown sort column {col}");
        }
    }
    if let Some((a, b)) = opts.select {
        queue.select_range(a, b);
    }

    // Queue page: filter bar (status dropdown + stat line) and the view, or an empty state.
    let stat = gtk4::Label::new(None);
    stat.add_css_class("stat");
    stat.set_hexpand(true);
    stat.set_xalign(1.0);
    let update_stat = {
        let (stat, sel) = (stat.clone(), queue.selection.clone());
        move || {
            let n = sel.n_items();
            let picked = sel.selection().size();
            stat.set_text(&format!("{n} ROWS \u{00B7} {picked} SELECTED"));
        }
    };
    update_stat();
    queue.selection.connect_selection_changed({
        let u = update_stat.clone();
        move |_, _, _| u()
    });
    queue.selection.connect_items_changed({
        let u = update_stat.clone();
        move |_, _, _, _| u()
    });

    let dropdown = gtk4::DropDown::from_strings(&[
        "Status: All",
        "Status: Good",
        "Status: Review",
        "Status: Not found",
        "Status: Error",
        "Status: Unsearched",
        "Status: Flagged",
    ]);
    {
        let q = queue.clone();
        dropdown.connect_selected_notify(move |d| {
            let s = d.selected();
            q.set_status_filter((s > 0).then(|| Status::from_u32(s - 1)));
        });
    }
    let filter_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    filter_bar.set_margin_bottom(8);
    filter_bar.append(&dropdown);
    filter_bar.append(&stat);

    let queue_body = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    queue_body.append(&filter_bar);
    let empty = gtk4::Label::new(Some("Nothing queued.\nDrop images here to start."));
    empty.add_css_class("note");
    empty.set_justify(gtk4::Justification::Center);
    empty.set_vexpand(true);
    let queue_stack = gtk4::Stack::new();
    queue_stack.add_named(&queue.root, Some("rows"));
    queue_stack.add_named(&empty, Some("empty"));
    queue_stack.set_visible_child_name(if opts.rows == 0 { "empty" } else { "rows" });
    queue_stack.set_vexpand(true);
    queue_body.append(&queue_stack);

    let stack = gtk4::Stack::new();
    stack.add_named(&page("Queue.", &queue_body), Some("queue"));
    stack.add_named(&page("Review.", &review.root), Some("review"));
    stack.set_vexpand(true);
    stack.set_visible_child_name(&opts.tab);

    let menubar = gtk4::PopoverMenuBar::from_model(Some(&menu_model()));

    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    top.add_css_class("topbar");
    let mark = gtk4::Label::new(Some("HATATE"));
    mark.add_css_class("wordmark");
    top.append(&mark);
    let tabs_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    tabs_box.add_css_class("linked");
    let mut tabs: Vec<gtk4::ToggleButton> = Vec::new();
    for (name, label) in [("queue", "QUEUE"), ("review", "REVIEW")] {
        let b = gtk4::ToggleButton::with_label(label);
        b.add_css_class("tab");
        b.set_active(opts.tab == name);
        if let Some(f) = tabs.first() {
            b.set_group(Some(f));
        }
        let stack = stack.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                stack.set_visible_child_name(name);
            }
        });
        tabs_box.append(&b);
        tabs.push(b);
    }
    top.append(&tabs_box);

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&menubar);
    root.append(&top);
    root.append(&stack);

    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title("Hatate (GTK spike)")
        .content(&root)
        .build();
    install_actions(app, &win, &queue);
    match opts.size {
        Some((w, h)) => win.set_default_size(w, h),
        None => win.set_default_size(1, 1),
    }
    win.present();

    let ui = Ui {
        app: app.clone(),
        win: win.clone(),
        stack,
        queue,
        review,
        dropdown,
        menubar,
        tabs,
        thumbs,
    };
    drive(ui, opts.clone(), start, load_ms);
}

/// Resolves after `n` frame-clock ticks of `w`.
async fn frames(w: &gtk4::Widget, n: u32) {
    let (tx, rx) = async_channel::bounded::<()>(1);
    let seen = Cell::new(0u32);
    w.add_tick_callback(move |_, _| {
        seen.set(seen.get() + 1);
        if seen.get() >= n {
            let _ = tx.try_send(());
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    let _ = rx.recv().await;
}

/// Frame-driven state machine: settle, then measure; then hand over to [`finish`].
fn drive(ui: Ui, opts: Options, start: Instant, load_ms: f64) {
    let widget = ui.win.clone().upcast::<gtk4::Widget>();
    let driven = opts.measure
        || opts.shot.is_some()
        || opts.tab_order
        || opts.audit
        || opts.listen.is_some();
    if !driven {
        // Interactive run: still print the first-frame time once.
        widget.add_tick_callback(first_frame_reporter(start, load_ms));
        return;
    }
    let frames_seen = Rc::new(Cell::new(0u32));
    let proc_ms = Rc::new(RefCell::new(Vec::<f64>::new()));
    let interval_ms = Rc::new(RefCell::new(Vec::<f64>::new()));
    let last_frame_us = Rc::new(Cell::new(0i64));
    let update_at = Rc::new(Cell::new(None::<Instant>));
    let fling_active = Rc::new(Cell::new(false));
    let first_frame_ms = Rc::new(Cell::new(None::<f64>));
    let queue_scroller = ui.queue.scroller.clone();
    let queue_sel = ui.queue.selection.clone();
    let queue_cols = ui.queue.view.clone();

    if let Some(clock) = widget.frame_clock() {
        let ua = update_at.clone();
        clock.connect_update(move |_| ua.set(Some(Instant::now())));
        let (ua, pm, fa, ff, fr) = (
            update_at.clone(),
            proc_ms.clone(),
            fling_active.clone(),
            first_frame_ms.clone(),
            frames_seen.clone(),
        );
        clock.connect_after_paint(move |_| {
            if ff.get().is_none() {
                ff.set(Some(start.elapsed().as_secs_f64() * 1e3));
            }
            fr.set(fr.get() + 1);
            if let (true, Some(t)) = (fa.get(), ua.get()) {
                pm.borrow_mut().push(t.elapsed().as_secs_f64() * 1e3);
            }
        });
    }

    let opts2 = opts.clone();
    let (fl, im, lf, fa, ff, pm) = (
        frames_seen.clone(),
        interval_ms.clone(),
        last_frame_us.clone(),
        fling_active.clone(),
        first_frame_ms.clone(),
        proc_ms,
    );
    let phase = Rc::new(Cell::new(0u8));
    let settle_until = Rc::new(Cell::new(0u32));
    let last_value = Cell::new(0.0f64);
    let sort_start = Cell::new(None::<Instant>);
    let sort_frames = Cell::new(0u32);
    let thumbs = ui.thumbs.clone();
    let ui2 = ui.clone();
    widget.add_tick_callback(move |w, clock| {
        let n = fl.get();
        let adj = queue_scroller.vadjustment();
        match phase.get() {
            // 0: wait for a settled first frame
            0 if n >= 20 => {
                let (rss, hwm) = measure::rss_kib();
                if opts2.measure {
                    println!("MEASURE first_frame_ms={:.1}", ff.get().unwrap_or(0.0));
                    println!("MEASURE store_and_view_build_ms={load_ms:.1}");
                    println!("MEASURE rss_after_load_kib={rss} peak_kib={hwm}");
                    println!("MEASURE window_size={}x{}", w.width(), w.height());
                    println!(
                        "MEASURE renderer={} scheme={} thumbs={}",
                        w.native()
                            .and_then(|n| n.renderer())
                            .map_or("none".to_string(), |r| r.type_().name().to_string()),
                        opts2.scheme.name(),
                        opts2.thumbs.is_some()
                    );
                }
                phase.set(if opts2.measure && opts2.rows > 0 { 1 } else { 9 });
            }
            // 1: time a 5,000-row sort by FILE to the next painted frame
            1 => {
                sort_start.set(Some(Instant::now()));
                sort_frames.set(n);
                queue_sel.unselect_all();
                queue_cols.sort_by_column(
                    queue_cols
                        .columns()
                        .item(1)
                        .and_downcast_ref::<gtk4::ColumnViewColumn>(),
                    gtk4::SortType::Descending,
                );
                phase.set(2);
            }
            2 if n > sort_frames.get() + 1 => {
                if let Some(t) = sort_start.get() {
                    println!(
                        "MEASURE sort_file_5000_ms_to_next_frame={:.1}",
                        t.elapsed().as_secs_f64() * 1e3
                    );
                }
                queue_sel.select_all();
                sort_start.set(Some(Instant::now()));
                sort_frames.set(n);
                phase.set(3);
            }
            3 if n > sort_frames.get() + 1 => {
                if let Some(t) = sort_start.get() {
                    println!(
                        "MEASURE select_all_5000_ms_to_next_frame={:.1}",
                        t.elapsed().as_secs_f64() * 1e3
                    );
                }
                queue_sel.unselect_all();
                settle_until.set(n + 10);
                phase.set(4);
            }
            // 4: start the fling from the top
            4 if n >= settle_until.get() => {
                adj.set_value(0.0);
                last_value.set(0.0);
                lf.set(clock.frame_time());
                fa.set(true);
                phase.set(5);
            }
            // 5: constant-speed fling, 60,000 px/s, top to bottom
            5 => {
                let t = clock.frame_time();
                let dt = (t - lf.get()) as f64 / 1e6;
                if lf.get() != 0 {
                    im.borrow_mut().push(dt * 1e3);
                }
                lf.set(t);
                let target = (last_value.get() + 60_000.0 * dt).min(adj.upper() - adj.page_size());
                adj.set_value(target);
                last_value.set(target);
                if target >= adj.upper() - adj.page_size() - 1.0 {
                    fa.set(false);
                    phase.set(6);
                }
            }
            6 => {
                let iv = im.borrow();
                let pm = pm.borrow();
                let (si, sp) = (summarize(&iv), summarize(&pm));
                println!(
                    "MEASURE fling_frames={} fling_distance_px={:.0} speed_px_per_s=60000 thumbs={}",
                    si.n,
                    last_value.get(),
                    if thumbs.is_some() { "real" } else { "off" }
                );
                println!(
                    "MEASURE frame_interval_ms p50={:.1} p95={:.1} p99={:.1} max={:.1} over_16.7={} over_33.3={}",
                    si.p50,
                    si.p95,
                    si.p99,
                    si.max,
                    count_over(&iv, 16.7),
                    count_over(&iv, 33.3)
                );
                println!(
                    "MEASURE frame_processing_ms p50={:.1} p95={:.1} p99={:.1} max={:.1} over_16.7={} n={}",
                    sp.p50,
                    sp.p95,
                    sp.p99,
                    sp.max,
                    count_over(&pm, 16.7),
                    sp.n
                );
                let (rss, hwm) = measure::rss_kib();
                println!("MEASURE rss_after_fling_kib={rss} peak_kib={hwm}");
                if let Some(t) = &thumbs {
                    let s = t.stats();
                    let d = summarize(&s.decode_ms);
                    println!(
                        "MEASURE thumbs binds={} hits={} decoded={} failed={} evicted={} cached={} pending={}",
                        s.requested,
                        s.hits,
                        s.decoded,
                        s.failed,
                        s.evicted,
                        t.cached(),
                        t.pending()
                    );
                    println!(
                        "MEASURE thumb_decode_ms p50={:.1} p95={:.1} max={:.1} n={}",
                        d.p50, d.p95, d.max, d.n
                    );
                }
                phase.set(9);
            }
            9 => {
                let (ui, opts) = (ui2.clone(), opts2.clone());
                glib::spawn_future_local(finish(ui, opts));
                return glib::ControlFlow::Break;
            }
            _ => {}
        }
        glib::ControlFlow::Continue
    });
}

fn first_frame_reporter(
    start: Instant,
    load_ms: f64,
) -> impl Fn(&gtk4::Widget, &gdk::FrameClock) -> glib::ControlFlow + 'static {
    let done = Cell::new(false);
    move |_, _| {
        if !done.get() {
            done.set(true);
            println!(
                "first tick {:.1} ms after start (queue build {load_ms:.1} ms)",
                start.elapsed().as_secs_f64() * 1e3
            );
        }
        glib::ControlFlow::Break
    }
}

fn describe(w: &gtk4::Widget) -> String {
    let name = w.type_().name();
    let label = w
        .downcast_ref::<gtk4::Button>()
        .and_then(|b| b.label())
        .map(|l| format!(" \"{l}\""))
        .unwrap_or_default();
    let css = w.css_classes().join(".");
    // The pointer tells apart widgets that otherwise describe the same (list rows).
    format!("{name}{label} [{css}] @{:p}", w.as_ptr())
}

/// Walk the Tab chain with `child_focus`, the same call the toolkit makes for Tab.
fn print_tab_order(win: &adw::ApplicationWindow, page: &str) {
    let mut seen: Vec<String> = Vec::new();
    println!("TAB page={page}");
    if let Some(w) = GtkWindowExt::focus(win) {
        println!("TAB 0 (initial focus): {}", describe(&w));
    }
    for step in 1..=60 {
        if !win.child_focus(gtk4::DirectionType::TabForward) {
            println!("TAB step {step}: no widget accepted focus (chain ends)");
            break;
        }
        let d = GtkWindowExt::focus(win)
            .map(|w| describe(&w))
            .unwrap_or_default();
        println!("TAB {step}: {d}");
        if seen.contains(&d) {
            break;
        }
        seen.push(d);
    }
}

/// Every descendant of `w`, depth first.
fn descendants(w: &gtk4::Widget) -> Vec<gtk4::Widget> {
    let mut out = Vec::new();
    let mut child = w.first_child();
    while let Some(c) = child {
        out.push(c.clone());
        out.extend(descendants(&c));
        child = c.next_sibling();
    }
    out
}

/// Render `w` (a window or a popover) to a texture with its own renderer.
fn render(w: &impl IsA<gtk4::Widget>) -> Result<gdk::Texture, String> {
    let w = w.upcast_ref::<gtk4::Widget>();
    let paintable = gtk4::WidgetPaintable::new(Some(w));
    let snap = gtk4::Snapshot::new();
    paintable.snapshot(&snap, f64::from(w.width()), f64::from(w.height()));
    let node = snap.to_node().ok_or("empty snapshot")?;
    let renderer = w.native().and_then(|n| n.renderer()).ok_or("no renderer")?;
    Ok(renderer.render_texture(&node, None))
}

fn pixels(tex: &gdk::Texture) -> Pixels {
    let mut d = gdk::TextureDownloader::new(tex);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = d.download_bytes();
    let (w, h) = (tex.width() as usize, tex.height() as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        rgba.extend_from_slice(&bytes[y * stride..y * stride + w * 4]);
    }
    Pixels { w, h, rgba }
}

fn save(tex: &gdk::Texture, path: &std::path::Path) {
    match tex.save_to_png(path) {
        Ok(()) => println!("SHOT {}", path.display()),
        Err(e) => eprintln!("shot failed: {e}"),
    }
}

/// `path` with `-suffix` before the extension.
fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("shot");
    path.with_file_name(format!("{stem}-{suffix}.png"))
}

/// The window with `popover` drawn over it just below `anchor`, as it appears on screen. The
/// popover's own surface is rendered separately for the pixel audit; this is for the eye.
fn composite(
    win: &adw::ApplicationWindow,
    popover: &gtk4::Popover,
    anchor: &gtk4::Widget,
) -> Result<gdk::Texture, String> {
    let snap = gtk4::Snapshot::new();
    gtk4::WidgetPaintable::new(Some(win)).snapshot(
        &snap,
        f64::from(win.width()),
        f64::from(win.height()),
    );
    let at = anchor
        .compute_point(win, &graphene::Point::new(0.0, anchor.height() as f32))
        .ok_or("anchor not in window")?;
    snap.save();
    snap.translate(&at);
    gtk4::WidgetPaintable::new(Some(popover)).snapshot(
        &snap,
        f64::from(popover.width()),
        f64::from(popover.height()),
    );
    snap.restore();
    let node = snap.to_node().ok_or("empty snapshot")?;
    let renderer = win
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    let area = graphene::Rect::new(0.0, 0.0, win.width() as f32, win.height() as f32);
    Ok(renderer.render_texture(&node, Some(&area)))
}

fn visible_popovers(win: &adw::ApplicationWindow) -> Vec<gtk4::Popover> {
    descendants(win.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Popover>().ok())
        .filter(|p| p.is_visible() && p.is_mapped())
        .collect()
}

fn rect_of(w: &gtk4::Widget, win: &adw::ApplicationWindow) -> Option<Rect> {
    let b = w.compute_bounds(win)?;
    Some(Rect {
        x: b.x().floor() as i64,
        y: b.y().floor() as i64,
        w: b.width().ceil() as i64,
        h: b.height().ceil() as i64,
    })
}

/// Name for a widget in audit lines: type, label and classes, no pointer.
fn audit_name(w: &gtk4::Widget) -> String {
    let label = w
        .downcast_ref::<gtk4::Button>()
        .and_then(|b| b.label())
        .map(|l| format!(" \"{l}\""))
        .unwrap_or_default();
    format!(
        "{}{label} [{}]",
        w.type_().name(),
        w.css_classes().join(".")
    )
}

/// Focus every mapped focusable widget of the current page in turn and measure the ring.
async fn audit_focus(ui: &Ui, scheme: Scheme, page: &str) {
    let win = &ui.win;
    let wid = win.clone().upcast::<gtk4::Widget>();
    let mut seen = std::collections::HashSet::new();
    let mut targets: Vec<gtk4::Widget> = Vec::new();
    for w in descendants(&wid) {
        if w.is_focusable() && w.is_mapped() && w.is_visible() && w.width() > 0 {
            let key = audit_name(&w);
            if seen.insert(key) {
                targets.push(w);
            }
        }
    }
    println!(
        "AUDIT focus page={page} scheme={} targets={}",
        scheme.name(),
        targets.len()
    );
    if page == "queue" {
        audit_rows(ui).await;
    }
    for t in targets {
        GtkWindowExt::set_focus(win, None::<&gtk4::Widget>);
        win.set_focus_visible(true);
        frames(&wid, 3).await;
        let Ok(before) = render(win).map(|t| pixels(&t)) else {
            continue;
        };
        t.grab_focus();
        frames(&wid, 14).await;
        let focused = GtkWindowExt::focus(win);
        let took = focused
            .as_ref()
            .is_some_and(|f| f == &t || f.is_ancestor(&t) || t.is_ancestor(f));
        let Ok(after) = render(win).map(|t| pixels(&t)) else {
            continue;
        };
        let Some(area) = rect_of(&t, win).map(|r| r.grow(6)) else {
            continue;
        };
        let name = audit_name(&t);
        match audit::ring_stats(&before, &after, area) {
            None if took => {
                println!(
                    "AUDIT focus {name}: NO_RING focus_taken=true {}",
                    verdict(false)
                );
            }
            None => println!("AUDIT focus {name}: SKIP (the widget did not take focus)"),
            Some(r) => println!(
                "AUDIT focus {name}: ring={} ring_px={} changed_px={} min_contrast_vs_beneath={:.2} beneath={} at={:?} widget={:?} focus_taken={took} {}",
                r.ring.hex(),
                r.ring_px,
                r.changed,
                r.min_contrast,
                r.worst_beneath.hex(),
                r.worst_at,
                area,
                verdict(r.min_contrast >= 3.0)
            ),
        }
    }
}

/// The focus ring on a list row, the way arrow-key navigation puts it there, on an unselected
/// and on a selected (stamp-filled) row.
async fn audit_rows(ui: &Ui) {
    let win = &ui.win;
    let wid = win.clone().upcast::<gtk4::Widget>();
    for (name, selected) in [("unselected", false), ("selected", true)] {
        ui.queue.selection.unselect_all();
        if selected {
            ui.queue.select_range(1, 1);
        }
        GtkWindowExt::set_focus(win, None::<&gtk4::Widget>);
        win.set_focus_visible(true);
        frames(&wid, 4).await;
        let Ok(before) = render(win).map(|t| pixels(&t)) else {
            continue;
        };
        ui.queue.view.grab_focus();
        ui.queue
            .view
            .scroll_to(1, None, gtk4::ListScrollFlags::FOCUS, None);
        frames(&wid, 4).await;
        let Some(row) = GtkWindowExt::focus(win) else {
            println!("AUDIT focus queue row ({name}): focus did not reach a row");
            continue;
        };
        let Ok(after) = render(win).map(|t| pixels(&t)) else {
            continue;
        };
        let Some(area) = rect_of(&row, win).map(|r| r.grow(4)) else {
            continue;
        };
        match audit::ring_stats(&before, &after, area) {
            None => println!(
                "AUDIT focus queue row ({name}) {}: NO_RING {}",
                audit_name(&row),
                verdict(false)
            ),
            Some(r) => println!(
                "AUDIT focus queue row ({name}) {}: ring={} ring_px={} changed_px={} min_contrast_vs_beneath={:.2} beneath={} {}",
                audit_name(&row),
                r.ring.hex(),
                r.ring_px,
                r.changed,
                r.min_contrast,
                r.worst_beneath.hex(),
                verdict(r.min_contrast >= 3.0)
            ),
        }
    }
    ui.queue.selection.unselect_all();
}

/// Open a dropdown, a menubar menu or the dialog and return the popovers now showing, each
/// with the widget it hangs from.
async fn open_surface(ui: &Ui, what: Open) -> Vec<(gtk4::Popover, gtk4::Widget)> {
    let wid = ui.win.clone().upcast::<gtk4::Widget>();
    match what {
        Open::Dropdown => {
            ui.dropdown.set_selected(2);
            // The dropdown's own toggle button opens its popover, as a click would.
            if let Some(b) = descendants(ui.dropdown.upcast_ref())
                .into_iter()
                .find_map(|w| w.downcast::<gtk4::ToggleButton>().ok())
            {
                b.emit_clicked();
            }
        }
        Open::Menubar => {
            // The first item of the bar is "File"; activating it opens its menu.
            if let Some(item) = descendants(ui.menubar.upcast_ref())
                .into_iter()
                .find(|w| w.type_().name() == "GtkPopoverMenuBarItem")
            {
                item.activate();
            }
        }
        Open::Dialog => {
            ui.queue.select_range(0, 2);
            confirm_dialog(ui.queue.selected_count()).present(Some(&ui.win));
        }
    }
    frames(&wid, 12).await;
    visible_popovers(&ui.win)
        .into_iter()
        .map(|p| {
            let anchor = p.parent().unwrap_or_else(|| wid.clone());
            (p, anchor)
        })
        .collect()
}

/// Everything after the measurement run: fonts report, window state, focus, open surfaces,
/// audits, Tab order, shots. Always ends by quitting the application.
async fn finish(ui: Ui, opts: Options) {
    let win = ui.win.clone();
    let wid = win.clone().upcast::<gtk4::Widget>();
    let scheme = opts.scheme;

    // Fonts: what Pango resolved, after the run's own font route.
    if opts.fonts.is_some() || opts.measure || opts.audit {
        for l in fonts::report().lines(opts.font_route) {
            println!("{l}");
        }
    }

    // Wait for the visible thumbnails (bounded) so a shot shows images, not empty frames.
    if let Some(t) = &ui.thumbs {
        let mut waited = 0;
        while t.pending() > 0 && waited < 600 {
            frames(&wid, 1).await;
            waited += 1;
        }
        let s = t.stats();
        println!(
            "THUMBS settled after {waited} frames: binds={} hits={} decoded={} failed={} cached={}",
            s.requested,
            s.hits,
            s.decoded,
            s.failed,
            t.cached()
        );
    }

    // Window state. Under Xvfb with no window manager nothing ever activates the window, so
    // report what GTK says and then set the state the shot is about.
    println!(
        "WINDOW is_active={} backdrop_native={}",
        win.is_active(),
        win.state_flags().contains(gtk4::StateFlags::BACKDROP)
    );
    if opts.inactive {
        win.set_state_flags(gtk4::StateFlags::BACKDROP, false);
    } else {
        win.unset_state_flags(gtk4::StateFlags::BACKDROP);
    }
    println!(
        "WINDOW forced backdrop={} (inactive shot requested: {})",
        win.state_flags().contains(gtk4::StateFlags::BACKDROP),
        opts.inactive
    );

    // Focus a widget so its ring is in the shot.
    if let Some(f) = &opts.focus {
        win.set_focus_visible(true);
        let target: Option<gtk4::Widget> = match f.as_str() {
            "queue" => {
                ui.stack.set_visible_child_name("queue");
                // Focus row 1 as arrow-key navigation would; optionally select it too.
                let flags = if opts.select_then_focus {
                    gtk4::ListScrollFlags::FOCUS | gtk4::ListScrollFlags::SELECT
                } else {
                    gtk4::ListScrollFlags::FOCUS
                };
                ui.queue.view.grab_focus();
                ui.queue.view.scroll_to(1, None, flags, None);
                None
            }
            "tab" => Some(ui.tabs[0].clone().upcast()),
            "canvas" => {
                ui.stack.set_visible_child_name("review");
                Some(ui.review.canvas.clone().upcast())
            }
            "slider" => {
                ui.stack.set_visible_child_name("review");
                Some(ui.review.slider.clone().upcast())
            }
            "mode" => {
                ui.stack.set_visible_child_name("review");
                ui.review
                    .mode_buttons
                    .get(1)
                    .map(|(_, b)| b.clone().upcast())
            }
            "dropdown" => Some(ui.dropdown.clone().upcast()),
            other => {
                eprintln!("--focus: unknown target {other}");
                None
            }
        };
        frames(&wid, 3).await;
        if let Some(t) = target {
            t.grab_focus();
        }
        frames(&wid, 4).await;
        println!(
            "FOCUS target={f} now_on={}",
            GtkWindowExt::focus(&win).map_or("nothing".to_string(), |w| audit_name(&w))
        );
    }

    // Focus sweep first: it moves focus around, so it runs before anything is opened.
    if opts.audit {
        for page in ["queue", "review"] {
            ui.stack.set_visible_child_name(page);
            frames(&wid, 4).await;
            audit_focus(&ui, scheme, page).await;
        }
        ui.stack.set_visible_child_name(&opts.tab);
        GtkWindowExt::set_focus(&win, None::<&gtk4::Widget>);
        frames(&wid, 3).await;
    }

    // Open surfaces.
    let mut popovers = Vec::new();
    let mut under: Option<Pixels> = None;
    if let Some(what) = opts.open {
        if what == Open::Dialog {
            // The same window without the dialog, for the shadow audit.
            ui.queue.select_range(0, 2);
            frames(&wid, 6).await;
            under = render(&win).ok().map(|t| pixels(&t));
        }
        popovers = open_surface(&ui, what).await;
        println!("OPEN {what:?}: popovers_visible={}", popovers.len());
    }
    if std::env::var_os("HATATE_DUMP_TREE").is_some() {
        for w in descendants(&wid) {
            let mut depth = 0;
            let mut p = w.parent();
            while let Some(x) = p {
                depth += 1;
                p = x.parent();
            }
            println!(
                "TREE {}{} <{}> [{}] mapped={}",
                "  ".repeat(depth),
                w.type_().name(),
                w.css_name(),
                w.css_classes().join("."),
                w.is_mapped()
            );
        }
    }

    // Surface audits.
    if opts.audit {
        let pal = theme::palette(scheme);
        for (p, _) in &popovers {
            let name = audit_name(p.upcast_ref());
            match render(p).map(|t| pixels(&t)) {
                Ok(px) => match audit::popover_flatness(&px) {
                    Some(f) => println!(
                        "AUDIT popover {name}: surface={}x{} body={}x{} corners_opaque={:?} square={} partial_alpha_px={} shadowless={} {}",
                        px.w,
                        px.h,
                        f.body.w,
                        f.body.h,
                        f.corners_opaque,
                        f.square(),
                        f.partial_alpha_px,
                        f.shadowless(),
                        verdict(f.square() && f.shadowless())
                    ),
                    None => println!(
                        "AUDIT popover {name}: nothing opaque rendered {}",
                        verdict(false)
                    ),
                },
                Err(e) => println!("AUDIT popover {name}: render failed: {e}"),
            }
        }
        if let (Some(Open::Dialog), Some(under)) = (opts.open, &under) {
            if let Ok(tex) = render(&win) {
                let with = pixels(&tex);
                // The card is the `sheet` node with the `background` class.
                let sheets: Vec<gtk4::Widget> = descendants(&wid)
                    .into_iter()
                    .filter(|w| w.css_name() == "sheet" && w.has_css_class("background"))
                    .collect();
                println!("AUDIT dialog cards_found={}", sheets.len());
                for s in sheets {
                    if let Some(r) = rect_of(&s, &win) {
                        let f = audit::card_flatness(
                            under,
                            &with,
                            r,
                            &[pal.ink_28.over(pal.raised)],
                            16,
                        );
                        println!(
                            "AUDIT dialog card={}x{}+{}+{} corners_square={:?} shadow_px={} {}",
                            r.w,
                            r.h,
                            r.x,
                            r.y,
                            f.corners_square,
                            f.shadow_px,
                            verdict(f.square() && f.shadowless())
                        );
                    }
                }
            }
        }
    }

    // `--listen`: keep running so an external keyboard (xdotool on the private Xvfb) can drive
    // the real shortcuts; every action and focus change is logged.
    if let Some(secs) = opts.listen {
        {
            let w = win.clone();
            win.connect_focus_widget_notify(move |_| {
                if let Some(f) = GtkWindowExt::focus(&w) {
                    println!("FOCUS {}", describe(&f));
                }
            });
        }
        win.present();
        println!("LISTEN ready for {secs}s");
        glib::timeout_future_seconds(secs as u32).await;
    }

    if opts.tab_order {
        GtkWindowExt::set_focus(&win, None::<&gtk4::Widget>);
        print_tab_order(
            &win,
            ui.stack.visible_child_name().as_deref().unwrap_or("?"),
        );
    }

    if let Some(path) = &opts.shot {
        frames(&wid, 2).await;
        match render(&win) {
            Ok(tex) => {
                if let (Some((p, anchor)), Some(_)) = (popovers.first(), opts.open) {
                    // Window with the popover over it, plus the popover's own surface.
                    match composite(&win, p, anchor) {
                        Ok(t) => save(&t, path),
                        Err(e) => eprintln!("composite failed: {e}"),
                    }
                    if let Ok(t) = render(p) {
                        save(&t, &with_suffix(path, "popover"));
                    }
                } else {
                    save(&tex, path);
                }
            }
            Err(e) => eprintln!("shot failed: {e}"),
        }
    }
    ui.app.quit();
}
