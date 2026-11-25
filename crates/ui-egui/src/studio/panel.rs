//! The PAULGA Studio panel: how the connection is doing, and the switch that turns it on and off.
//! Everything it does goes through `studio.connect` and `studio.disconnect`.

use egui::RichText;
use serde_json::json;

use super::Phase;
use crate::CadApp;

/// One name and value line; the value is read out with its name.
fn row(ui: &mut egui::Ui, name: &str, value: &str) {
    let t = crate::theme::Tokens::get();
    let name = ui.label(RichText::new(name).color(t.text_dim));
    ui.label(RichText::new(value).color(t.text)).labelled_by(name.id);
    ui.end_row();
}

/// The panel body.
pub fn section(app: &mut CadApp, ui: &mut egui::Ui) {
    let mut act = None;
    // One margin for everything, so wrapped lines keep the indent of their first.
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 8)).show(ui, |ui| body(app, ui, &mut act));
    if let Some(command) = act
        && let Err(error) = app.run(command, json!({}))
    {
        app.set_status(error);
    }
}

fn body(app: &CadApp, ui: &mut egui::Ui, act: &mut Option<&'static str>) {
    let t = crate::theme::Tokens::get();
    let Some(link) = app.studio.clone() else {
        ui.label(RichText::new(crate::i18n::t(super::UNAVAILABLE)).color(t.text_dim));
        return;
    };
    let s = link.snapshot();
    // Words carry the state; the colour only repeats it.
    let color = match s.phase {
        Phase::Connected => t.text,
        Phase::Refused => t.warn,
        _ => t.text_dim,
    };
    ui.label(RichText::new(crate::i18n::t(s.phase.label())).color(color).strong());
    if let Some(note) = &s.note {
        ui.label(RichText::new(crate::i18n::t(note)).color(t.text_dim));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (label, command) = if s.wanted { (crate::tl!("Disconnect"), "studio.disconnect") } else { (crate::tl!("Connect"), "studio.connect") };
        if ui.button(label).clicked() {
            *act = Some(command);
        }
        if s.wanted && matches!(s.phase, Phase::Searching | Phase::Refused) && ui.button(crate::tl!("Try Again")).clicked() {
            *act = Some("studio.connect");
        }
    });
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    egui::Grid::new("studio-panel-grid").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        let none = crate::tl!("None");
        row(ui, crate::tl!("Drawing"), s.document.name.as_deref().unwrap_or(none));
        if let Some(version) = &s.studio_version {
            row(ui, crate::tl!("Studio version"), version);
        }
        if let Some(port) = s.port {
            row(ui, crate::tl!("Port"), &port.to_string());
        }
        row(ui, crate::tl!("Tools run"), &s.tools_run.to_string());
        if let Some(tool) = &s.last_tool {
            row(ui, crate::tl!("Last tool"), tool);
        }
    });
    ui.add_space(8.0);
    ui.label(
        RichText::new(crate::tl!("PAULGA Studio on this computer can read the open drawing and run PAULGA CAD commands in it. Each change is an undo step."))
            .color(t.text_dim),
    );
}
