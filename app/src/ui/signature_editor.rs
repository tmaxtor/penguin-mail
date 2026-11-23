//! Shared editor for account signatures and custom sender signatures.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gtk::{gdk, gio, glib};
use mailrs_domain::translate::{gettext, with_reason};

use crate::compose::html_signature::{prefers_source, visual_body, visual_template};
use crate::richtext::RichBody;
use crate::settings::ComposeFormat;

use super::composer::{editor::Editor, pictures::alt_from_file_name};

type Changed = Box<dyn Fn(String)>;

pub struct SignatureEditor {
    pub widget: gtk::Box,
    state: Rc<State>,
    source: gtk::ToggleButton,
}

struct State {
    editor: Rc<Editor>,
    raw: gtk::TextBuffer,
    in_source: Cell<bool>,
    loading: Cell<bool>,
    queued: Cell<bool>,
    original: RefCell<(String, RichBody)>,
    announced: RefCell<String>,
    changed: RefCell<Option<Changed>>,
}

impl State {
    fn load(&self, text: &str, in_source: bool) {
        self.loading.set(true);
        self.in_source.set(in_source);
        let mut body = RichBody::default();
        if in_source {
            self.raw.set_text(text);
        } else {
            self.editor.fill("", Some(&visual_body(text)), &[]);
            // Read back the buffer because it combines adjacent runs with the same style.
            body = self.editor.rich();
        }
        self.original.replace((text.to_string(), body));
        self.loading.set(false);
    }

    fn text(&self) -> String {
        if self.in_source.get() {
            return self
                .raw
                .text(&self.raw.start_iter(), &self.raw.end_iter(), false)
                .to_string();
        }
        let current = self.editor.rich();
        let original = self.original.borrow();
        if current == original.1 {
            original.0.clone()
        } else {
            visual_template(&current)
        }
    }

    fn announce(&self) {
        let text = self.text();
        if text == *self.announced.borrow() {
            return;
        }
        self.announced.replace(text.clone());
        if let Some(changed) = &*self.changed.borrow() {
            changed(text);
        }
    }

    fn queue(self: &Rc<Self>) {
        if self.loading.get() || self.queued.replace(true) {
            return;
        }
        let this = Rc::clone(self);
        glib::idle_add_local_once(move || {
            this.queued.set(false);
            this.announce();
        });
    }
}

impl SignatureEditor {
    pub fn new(text: &str, title: &str, help: &str) -> Self {
        let visual = text_view();
        let raw = text_view();
        for view in [&visual, &raw] {
            super::describe(view, title, help);
            view.set_tooltip_text(Some(help));
        }
        raw.set_monospace(true);
        let state = Rc::new(State {
            editor: Editor::new(&visual, ComposeFormat::Rich),
            raw: raw.buffer(),
            in_source: Cell::new(false),
            loading: Cell::new(false),
            queued: Cell::new(false),
            original: RefCell::default(),
            announced: RefCell::new(text.to_string()),
            changed: RefCell::default(),
        });
        let in_source = prefers_source(text);
        state.load(text, in_source);

        let bar = gtk::Box::builder()
            .spacing(4)
            .margin_start(6)
            .margin_end(6)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        let formatting = format_bar(&state, &visual);
        formatting.set_hexpand(true);
        formatting.set_visible(!in_source);
        bar.append(&formatting);
        let source = gtk::ToggleButton::builder()
            .label(mode_label(in_source))
            .active(in_source)
            .css_classes(["flat"])
            .build();
        bar.append(&source);
        let visual_scroll = scroll(&visual);
        let raw_scroll = scroll(&raw);
        visual_scroll.set_visible(!in_source);
        raw_scroll.set_visible(in_source);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&bar);
        widget.append(&visual_scroll);
        widget.append(&raw_scroll);

        for buffer in [visual.buffer(), raw.buffer()] {
            let weak = Rc::downgrade(&state);
            buffer.connect_changed(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.queue();
                }
            });
        }
        let weak = Rc::downgrade(&state);
        state.editor.connect_described(move || {
            if let Some(state) = weak.upgrade() {
                state.queue();
            }
        });
        let mode = Rc::clone(&state);
        source.connect_toggled(move |button| {
            let use_source = button.is_active();
            if use_source != mode.in_source.get() {
                mode.load(&mode.text(), use_source);
            }
            button.set_label(&mode_label(use_source));
            formatting.set_visible(!use_source);
            visual_scroll.set_visible(!use_source);
            raw_scroll.set_visible(use_source);
        });
        Self {
            widget,
            state,
            source,
        }
    }

    pub fn text(&self) -> String {
        self.state.text()
    }

    pub fn set_text(&self, text: &str) {
        let in_source = prefers_source(text);
        self.state.load(text, in_source);
        self.source.set_active(in_source);
        self.state.announce();
    }

    pub fn connect_changed(&self, changed: impl Fn(String) + 'static) {
        self.state.changed.replace(Some(Box::new(changed)));
    }
}

fn mode_label(in_source: bool) -> String {
    if in_source {
        gettext("Edit Visually")
    } else {
        gettext("Edit Source")
    }
}

fn text_view() -> gtk::TextView {
    gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(12)
        .right_margin(12)
        .accepts_tab(false)
        .build()
}

fn scroll(view: &gtk::TextView) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(view)
        .min_content_height(96)
        .max_content_height(220)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build()
}

fn format_bar(state: &Rc<State>, view: &gtk::TextView) -> gtk::Box {
    let bar = gtk::Box::builder()
        .spacing(2)
        .accessible_role(gtk::AccessibleRole::Toolbar)
        .build();
    super::name(&bar, &gettext("Signature Formatting"));
    for (icon, label, tag, key) in [
        (
            "format-text-bold-symbolic",
            gettext("Bold (Ctrl+B)"),
            "bold",
            "<Control>b",
        ),
        (
            "format-text-italic-symbolic",
            gettext("Italic (Ctrl+I)"),
            "italic",
            "<Control>i",
        ),
    ] {
        let button = gtk::ToggleButton::builder()
            .icon_name(icon)
            .tooltip_text(&label)
            .css_classes(["flat"])
            .build();
        super::name_with_shortcut(&button, &label);
        let apply = {
            let (weak, view) = (Rc::downgrade(state), view.downgrade());
            move || {
                let (Some(state), Some(view)) = (weak.upgrade(), view.upgrade()) else {
                    return;
                };
                state.editor.toggle(tag);
                view.grab_focus();
                state.queue();
            }
        };
        let clicked = apply.clone();
        button.connect_clicked(move |_| clicked());
        let keys = gtk::ShortcutController::new();
        keys.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(key),
            Some(gtk::CallbackAction::new(move |_, _| {
                apply();
                glib::Propagation::Stop
            })),
        ));
        view.add_controller(keys);
        let refresh = {
            let (weak, button) = (Rc::downgrade(state), button.downgrade());
            move || {
                if let (Some(state), Some(button)) = (weak.upgrade(), button.upgrade()) {
                    let style = state.editor.style_here();
                    button.set_active(if tag == "bold" {
                        style.bold
                    } else {
                        style.italic
                    });
                }
            }
        };
        let cursor = refresh.clone();
        view.buffer().connect_mark_set(move |_, _, _| cursor());
        let styles = refresh.clone();
        view.buffer().connect_apply_tag(move |_, _, _, _| styles());
        view.buffer()
            .connect_remove_tag(move |_, _, _, _| refresh());
        bar.append(&button);
    }
    let color = toolbar_button(&bar, "color-select-symbolic", &gettext("Text Color"));
    let (weak, anchor) = (Rc::downgrade(state), view.downgrade());
    color.connect_clicked(move |button| {
        let window = button.root().and_downcast::<gtk::Window>();
        let (weak, anchor) = (weak.clone(), anchor.clone());
        let initial = weak
            .upgrade()
            .and_then(|state| state.editor.style_here().color)
            .map(|[r, g, b]| {
                gdk::RGBA::new(
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                )
            });
        let dialog = gtk::ColorDialog::builder()
            .title(gettext("Text Color"))
            .with_alpha(false)
            .build();
        glib::spawn_future_local(async move {
            let Ok(color) = dialog
                .choose_rgba_future(window.as_ref(), initial.as_ref())
                .await
            else {
                return;
            };
            let channel = |value: f32| (value * 255.0).round() as u8;
            apply_color(
                &weak,
                &anchor,
                Some([
                    channel(color.red()),
                    channel(color.green()),
                    channel(color.blue()),
                ]),
            );
        });
    });
    let reset = toolbar_button(&bar, "edit-clear-symbolic", &gettext("Reset Text Color"));
    let (weak, anchor) = (Rc::downgrade(state), view.downgrade());
    reset.connect_clicked(move |_| apply_color(&weak, &anchor, None));
    let picture = toolbar_button(&bar, "image-x-generic-symbolic", &gettext("Insert Image"));
    let (weak, anchor) = (Rc::downgrade(state), view.downgrade());
    picture.connect_clicked(move |button| {
        let window = button.root().and_downcast::<gtk::Window>();
        let (weak, anchor) = (weak.clone(), anchor.clone());
        glib::spawn_future_local(async move {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&gettext("Images")));
            filter.add_mime_type("image/*");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder()
                .title(gettext("Insert Image"))
                .filters(&filters)
                .build();
            let Ok(file) = dialog.open_future(window.as_ref()).await else {
                return;
            };
            let result = load_picture(&file).await;
            let (Some(state), Some(view)) = (weak.upgrade(), anchor.upgrade()) else {
                return;
            };
            if !view.is_mapped() || state.in_source.get() {
                return;
            }
            match result {
                Ok((src, data)) => {
                    let name = file.basename().unwrap_or_default();
                    let alt = alt_from_file_name(&name.to_string_lossy());
                    state.editor.insert_picture(&src, &alt, &data);
                    state.queue();
                    view.grab_focus();
                }
                Err(reason) => {
                    let alert = adw::AlertDialog::new(
                        Some(&gettext("Could Not Insert Image")),
                        Some(&reason),
                    );
                    alert.add_response("close", &gettext("Close"));
                    alert.set_close_response("close");
                    alert.present(Some(&view));
                }
            }
        });
    });
    bar
}

fn toolbar_button(bar: &gtk::Box, icon: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(label)
        .css_classes(["flat"])
        .build();
    super::name(&button, label);
    bar.append(&button);
    button
}

fn apply_color(
    weak: &std::rc::Weak<State>,
    anchor: &glib::WeakRef<gtk::TextView>,
    color: Option<[u8; 3]>,
) {
    if let (Some(state), Some(view)) = (weak.upgrade(), anchor.upgrade())
        && view.is_mapped()
        && !state.in_source.get()
    {
        state.editor.color(color);
        view.grab_focus();
        state.queue();
    }
}

async fn load_picture(file: &gio::File) -> Result<(String, glib::Bytes), String> {
    const MAX_BYTES: usize = 8 * 1024 * 1024;
    let file_error =
        |err: glib::Error| with_reason(&gettext("Could not read the file: {reason}"), &err, &[]);
    let info = file
        .query_info_future(
            "standard::size",
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await
        .map_err(file_error)?;
    let too_large = || gettext("This image is too large for a signature.");
    if info.size() > MAX_BYTES as i64 {
        return Err(too_large());
    }
    let (bytes, _) = file.load_contents_future().await.map_err(file_error)?;
    if bytes.len() > MAX_BYTES
        || (super::texture::decoded_in_process(&bytes) && !crate::photos::fits(&bytes))
    {
        return Err(too_large());
    }
    let bytes = glib::Bytes::from_owned(bytes);
    let Some(texture) = super::texture::decode(bytes.clone()).await else {
        return Err(gettext("This file could not be read as an image."));
    };
    let (mime, data) = match bytes.as_ref() {
        b if b.starts_with(b"\x89PNG\r\n\x1a\n") => ("image/png", bytes),
        b if b.starts_with(b"\xff\xd8") => ("image/jpeg", bytes),
        _ => ("image/png", texture.save_to_png_bytes()),
    };
    if data.len() > MAX_BYTES {
        return Err(too_large());
    }
    let encoded = STANDARD.encode(&data);
    Ok((format!("data:{mime};base64,{encoded}"), data))
}
