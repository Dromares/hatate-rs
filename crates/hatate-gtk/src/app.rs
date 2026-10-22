//! Window assembly and the headless driver (shots, measurements, Tab order).

use crate::fixtures;
use crate::measure::{self, count_over, summarize};
use crate::options::Options;
use crate::queue::{self, Queue};
use crate::review::{self, Review};
use crate::theme;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

pub const APP_ID: &str = "org.hatate.GtkSpike";

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
    app.run_with_args::<&str>(&[])
}

fn build(app: &adw::Application, opts: &Options, start: Instant) {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    theme::install();

    let (gl, gr) = fixtures::sample_pair();
    let review = Review::new(
        load_side(&opts.left, gl),
        load_side(&opts.right, gr),
        opts.mode,
    );

    let t_load = Instant::now();
    let store = queue::build_store(opts.rows);
    let queue = Queue::new(&store);
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

    // Queue page: stat line + the view, or an empty state when there are no rows.
    let stat = gtk4::Label::new(None);
    stat.add_css_class("stat");
    stat.set_xalign(0.0);
    stat.set_margin_bottom(8);
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
    let queue_body = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    queue_body.append(&stat);
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

    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    top.add_css_class("topbar");
    let mark = gtk4::Label::new(Some("HATATE"));
    mark.add_css_class("wordmark");
    top.append(&mark);
    let tabs = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    tabs.add_css_class("linked");
    let mut first: Option<gtk4::ToggleButton> = None;
    for (name, label) in [("queue", "QUEUE"), ("review", "REVIEW")] {
        let b = gtk4::ToggleButton::with_label(label);
        b.add_css_class("tab");
        b.set_active(opts.tab == name);
        if let Some(f) = &first {
            b.set_group(Some(f));
        } else {
            first = Some(b.clone());
        }
        let stack = stack.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                stack.set_visible_child_name(name);
            }
        });
        tabs.append(&b);
    }
    top.append(&tabs);

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&top);
    root.append(&stack);

    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title("Hatate (GTK spike)")
        .content(&root)
        .build();
    match opts.size {
        Some((w, h)) => win.set_default_size(w, h),
        None => win.set_default_size(1, 1),
    }
    win.present();

    let clock_widget = win.clone().upcast::<gtk4::Widget>();
    drive(
        app,
        &win,
        &clock_widget,
        &queue,
        opts.clone(),
        start,
        load_ms,
    );
}

/// Frame-driven state machine: settle, then shoot / measure / walk the Tab chain.
fn drive(
    app: &adw::Application,
    win: &adw::ApplicationWindow,
    widget: &gtk4::Widget,
    queue: &Queue,
    opts: Options,
    start: Instant,
    load_ms: f64,
) {
    if !(opts.measure || opts.shot.is_some() || opts.tab_order) {
        // Interactive run: still print the first-frame time once.
        widget.add_tick_callback(first_frame_reporter(start, load_ms));
        return;
    }
    let frames = Rc::new(Cell::new(0u32));
    let proc_ms = Rc::new(RefCell::new(Vec::<f64>::new()));
    let interval_ms = Rc::new(RefCell::new(Vec::<f64>::new()));
    let last_frame_us = Rc::new(Cell::new(0i64));
    let update_at = Rc::new(Cell::new(None::<Instant>));
    let fling_active = Rc::new(Cell::new(false));
    let first_frame_ms = Rc::new(Cell::new(None::<f64>));
    let (win, app) = (win.clone(), app.clone());
    let queue_scroller = queue.scroller.clone();
    let queue_sel = queue.selection.clone();
    let queue_cols = queue.view.clone();

    if let Some(clock) = widget.frame_clock() {
        let ua = update_at.clone();
        clock.connect_update(move |_| ua.set(Some(Instant::now())));
        let (ua, pm, fa, ff, fr) = (
            update_at.clone(),
            proc_ms.clone(),
            fling_active.clone(),
            first_frame_ms.clone(),
            frames.clone(),
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
        frames.clone(),
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
                }
                phase.set(if opts2.measure && opts2.rows > 0 { 1 } else { 9 });
            }
            // 1: time a 5,000-row sort by FILE to the next painted frame
            1 => {
                sort_start.set(Some(Instant::now()));
                sort_frames.set(n);
                queue_sel.unselect_all();
                queue_cols.sort_by_column(
                    queue_cols.columns().item(1).and_downcast_ref::<gtk4::ColumnViewColumn>(),
                    gtk4::SortType::Descending,
                );
                phase.set(2);
            }
            2 if n > sort_frames.get() + 1 => {
                if let Some(t) = sort_start.get() {
                    println!("MEASURE sort_file_5000_ms_to_next_frame={:.1}", t.elapsed().as_secs_f64() * 1e3);
                }
                queue_sel.select_all();
                sort_start.set(Some(Instant::now()));
                sort_frames.set(n);
                phase.set(3);
            }
            3 if n > sort_frames.get() + 1 => {
                if let Some(t) = sort_start.get() {
                    println!("MEASURE select_all_5000_ms_to_next_frame={:.1}", t.elapsed().as_secs_f64() * 1e3);
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
                    "MEASURE fling_frames={} fling_distance_px={:.0} speed_px_per_s=60000",
                    si.n, last_value.get()
                );
                println!(
                    "MEASURE frame_interval_ms p50={:.1} p95={:.1} p99={:.1} max={:.1} over_16.7={} over_33.3={}",
                    si.p50, si.p95, si.p99, si.max, count_over(&iv, 16.7), count_over(&iv, 33.3)
                );
                println!(
                    "MEASURE frame_processing_ms p50={:.1} p95={:.1} p99={:.1} max={:.1} over_16.7={}",
                    sp.p50, sp.p95, sp.p99, sp.max, count_over(&pm, 16.7)
                );
                let (rss, hwm) = measure::rss_kib();
                println!("MEASURE rss_after_fling_kib={rss} peak_kib={hwm}");
                phase.set(9);
            }
            9 => {
                if opts2.tab_order {
                    print_tab_order(&win);
                }
                if let Some(path) = &opts2.shot {
                    match shoot(&win, path) {
                        Ok(()) => println!("SHOT {}", path.display()),
                        Err(e) => eprintln!("shot failed: {e}"),
                    }
                }
                app.quit();
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
fn print_tab_order(win: &adw::ApplicationWindow) {
    let mut seen: Vec<String> = Vec::new();
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

/// Render the window's current contents to a PNG without any display capture.
fn shoot(win: &adw::ApplicationWindow, path: &std::path::Path) -> Result<(), String> {
    let (w, h) = (win.width(), win.height());
    let paintable = gtk4::WidgetPaintable::new(Some(win));
    let snap = gtk4::Snapshot::new();
    paintable.snapshot(&snap, f64::from(w), f64::from(h));
    let node = snap.to_node().ok_or("empty snapshot")?;
    let renderer = win
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    let tex = renderer.render_texture(&node, None);
    tex.save_to_png(path).map_err(|e| e.to_string())
}
