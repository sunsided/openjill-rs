#![forbid(unsafe_code)]

use anyhow::Result;

fn main() -> Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "openjill-ui-demo",
        options,
        Box::new(|creation_context| {
            creation_context
                .egui_ctx
                .set_theme(egui::ThemePreference::System);
            Ok(Box::<DemoApp>::default())
        }),
    )?;
    Ok(())
}

#[derive(Default)]
struct DemoApp;

impl eframe::App for DemoApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        context.set_theme(egui::ThemePreference::System);
    }

    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}
}
