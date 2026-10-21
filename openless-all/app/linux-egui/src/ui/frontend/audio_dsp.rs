use eframe::egui;
use openless_core::{AudioDspConfig, AudioDspStage, NoiseReduction};
use openless_linux_egui::{tr_l10n, Lang};

pub fn settings(ui: &mut egui::Ui, config: &mut AudioDspConfig, lang: Lang, enabled: bool) -> bool {
    let before = *config;
    let label = |key| tr_l10n(lang, key);
    egui::CollapsingHeader::new(label("settings.audioDsp.Speech_enhancement_settings"))
        .id_salt("audio-dsp-settings").default_open(false).show(ui, |ui| {
        if !enabled { ui.small(label("settings.audioDsp.disabledHint")); }
        ui.add_enabled_ui(enabled, |ui| {
        ui.small(label("settings.audioDsp.RNNoise_is_lightweight_PureVox_uses_more_CPU_Changes_apply_to_the_next_recording"));
        ui.add_space(8.0);
        ui.strong(label("settings.audioDsp.noiseStage"));
        egui::ComboBox::from_id_salt("audio-dsp-algorithm")
            .width(ui.available_width().min(280.0))
            .selected_text(match config.noise_reduction {
                NoiseReduction::Off => label("settings.audioDsp.Noise_reduction_off"),
                NoiseReduction::Rnnoise => "RNNoise",
                NoiseReduction::Purevox => "PureVox 6",
                NoiseReduction::Spectral => label("settings.audioDsp.Spectral_subtraction"),
            })
            .show_ui(ui, |ui| {
                for (value, name) in [(NoiseReduction::Off, label("settings.audioDsp.Off")), (NoiseReduction::Rnnoise, "RNNoise"), (NoiseReduction::Purevox, "PureVox 6"), (NoiseReduction::Spectral, label("settings.audioDsp.Spectral_subtraction"))] {
                    ui.selectable_value(&mut config.noise_reduction, value, name);
                }
            });
        if config.noise_reduction != NoiseReduction::Off {
            slider(ui, label("settings.audioDsp.Noise_reduction"), &mut config.noise_strength, 0..=100, "%");
        }
        ui.separator();
        ui.strong(label("settings.audioDsp.Input_gain"));
        slider(ui, label("settings.audioDsp.Input_gain_dB"), &mut config.gain_db, -30..=30, " dB");
        ui.checkbox(&mut config.agc_enabled, label("settings.audioDsp.Automatic_gain"));
        if config.agc_enabled {
            slider(ui, label("settings.audioDsp.Target_level"), &mut config.agc_target, 5..=90, "%");
            slider(ui, label("settings.audioDsp.Attack_speed"), &mut config.agc_attack, 1..=100, "");
            slider(ui, label("settings.audioDsp.Release_speed"), &mut config.agc_decay, 1..=100, "");
        }
        ui.separator();
        ui.strong(label("settings.audioDsp.Speech_gate"));
        ui.checkbox(&mut config.vad_enabled, label("settings.audioDsp.Speech_gate_does_not_stop_recording"));
        if config.vad_enabled {
            slider(ui, label("settings.audioDsp.Threshold_dB"), &mut config.vad_threshold_db, -80..=-10, " dB");
            ui.small(label("settings.audioDsp.A_high_threshold_can_remove_quiet_speech"));
        }
        ui.add_space(8.0);
        ui.collapsing(label("settings.audioDsp.Tone_and_room_adjustments"), |ui| {
            ui.small(label("settings.audioDsp.The_MicYou_comb_filter_can_color_speech_Start_at_0"));
            slider(ui, label("settings.audioDsp.Comb_filter"), &mut config.dereverb_level, 0..=100, "%");
            ui.checkbox(&mut config.equalizer_enabled, label("settings.audioDsp.10_band_equalizer"));
            if config.equalizer_enabled {
                slider(ui, label("settings.audioDsp.Preamp_dB"), &mut config.equalizer_preamp_db, -20..=20, " dB");
                let columns = if ui.available_width() >= 540.0 { 2 } else { 1 };
                egui::Grid::new("audio-dsp-eq").num_columns(columns).spacing([16.0, 8.0]).show(ui, |ui| {
                    let width = ((ui.available_width() - 16.0 * (columns - 1) as f32) / columns as f32).max(100.0);
                    for (index, (band, name)) in config.equalizer_bands_db.iter_mut().zip(["31 Hz", "63 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"]).enumerate() {
                        ui.push_id(index, |ui| ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.label(name);
                            ui.spacing_mut().slider_width = (width - 80.0).clamp(60.0, 220.0);
                            ui.add(egui::Slider::new(band, -12..=12).suffix(" dB"));
                        }));
                        if (index + 1) % columns == 0 { ui.end_row(); }
                    }
                });
            }
        });
        ui.collapsing(label("settings.audioDsp.Processing_order"), |ui| {
            let mut swap = None;
            for (i, stage) in config.processing_order.iter().enumerate() {
                let name = match stage {
                    AudioDspStage::NoiseReduction => label("settings.audioDsp.noiseStage"),
                    AudioDspStage::Dereverb => label("settings.audioDsp.combStage"),
                    AudioDspStage::Equalizer => label("settings.audioDsp.Equalizer"),
                    AudioDspStage::Amplifier => label("settings.audioDsp.Input_gain"),
                    AudioDspStage::Agc => label("settings.audioDsp.Automatic_gain"),
                    AudioDspStage::Vad => label("settings.audioDsp.Speech_gate"),
                };
                ui.push_id(i, |ui| ui.horizontal(|ui| {
                    ui.monospace(format!("{}", i + 1));
                    let active = match stage {
                        AudioDspStage::NoiseReduction => config.noise_reduction != NoiseReduction::Off && config.noise_strength > 0,
                        AudioDspStage::Dereverb => config.dereverb_level > 0,
                        AudioDspStage::Equalizer => config.equalizer_enabled,
                        AudioDspStage::Amplifier => config.gain_db != 0,
                        AudioDspStage::Agc => config.agc_enabled,
                        AudioDspStage::Vad => config.vad_enabled,
                    };
                    ui.add_enabled(active, egui::Label::new(name));
                    if ui.add_enabled(i > 0, egui::Button::new("↑")).on_hover_text(label("settings.audioDsp.moveUp")).clicked() { swap = Some((i, i - 1)); }
                    if ui.add_enabled(i + 1 < config.processing_order.len(), egui::Button::new("↓")).on_hover_text(label("settings.audioDsp.moveDown")).clicked() { swap = Some((i, i + 1)); }
                }));
            }
            if let Some((a, b)) = swap { config.processing_order.swap(a, b); }
        });
        ui.add_space(10.0);
        if ui.button(label("settings.audioDsp.Reset_enhancement_settings")).clicked() { *config = Default::default(); }
    });
        });
    before != *config
}

fn slider<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    title: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    suffix: &str,
) {
    let narrow = ui.available_width() < 420.0;
    if narrow {
        ui.label(title);
    }
    ui.horizontal(|ui| {
        if !narrow {
            ui.add_sized([160.0, 24.0], egui::Label::new(title).wrap());
        }
        ui.spacing_mut().slider_width = (ui.available_width() - 85.0).clamp(60.0, 240.0);
        ui.add(egui::Slider::new(value, range).suffix(suffix));
    });
    ui.add_space(4.0);
}
