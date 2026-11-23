//! A sender's signature choice, saved only when the dialog is accepted.

use std::rc::Rc;

use adw::prelude::*;
use mailrs_domain::translate::{fill, gettext};

use crate::app::App;
use crate::settings::{Change, SignatureMode};

pub(super) fn show(app: &Rc<App>, parent: &impl IsA<gtk::Widget>, account: &str, email: &str) {
    let settings = app.settings();
    let modes = [
        SignatureMode::Account,
        SignatureMode::Custom,
        SignatureMode::None,
    ];
    let selected = modes
        .iter()
        .position(|m| *m == settings.signature_mode(account, email))
        .unwrap_or(0) as u32;
    let title = fill(&gettext("Signature for {account}"), &[("account", email)]);
    let dialog = adw::AlertDialog::builder().heading(&title).build();
    let group = adw::PreferencesGroup::new();
    let labels = [
        gettext("Use Account Signature"),
        gettext("Custom Signature"),
        gettext("No signature"),
    ];
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let choice = adw::ComboRow::builder()
        .title(gettext("Signature"))
        .model(&gtk::StringList::new(&refs))
        .selected(selected)
        .build();
    group.add(&choice);
    let help = gettext(
        "Use Edit Source to write Markdown or HTML. <email> becomes the sending address, including its suffix. <domain> becomes its domain.",
    );
    let editor = crate::ui::signature_editor::SignatureEditor::new(
        settings.signature_text(account, email),
        &title,
        &help,
    );
    editor.widget.set_visible(selected == 1);
    let weak_editor = editor.widget.downgrade();
    choice.connect_selected_notify(move |choice| {
        if let Some(editor) = weak_editor.upgrade() {
            editor.set_visible(choice.selected() == 1);
        }
    });
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(
        &gtk::Label::builder()
            .label(&help)
            .wrap(true)
            .xalign(0.0)
            .max_width_chars(48)
            .build(),
    );
    content.append(&editor.widget);
    dialog.set_extra_child(Some(&content));
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("save", &gettext("Save"))]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    let (app, email) = (Rc::downgrade(app), email.to_string());
    dialog.connect_response(Some("save"), move |_, _| {
        let Some(app) = app.upgrade() else { return };
        app.change_settings(Change::SenderSignature {
            email: email.clone(),
            mode: modes[choice.selected() as usize],
            text: editor.text(),
        });
    });
    dialog.present(Some(parent));
}
