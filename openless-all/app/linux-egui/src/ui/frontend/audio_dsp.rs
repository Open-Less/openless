use eframe::egui;
use openless_core::{AudioDspConfig, AudioDspStage, NoiseReduction};
use openless_linux_egui::{tr_l10n, Lang};

pub fn settings(ui: &mut egui::Ui, config: &mut AudioDspConfig, lang: Lang) -> bool {
    let before = *config;
    let label = |key| tr_l10n(lang, key);
    ui.collapsing(label("settings.audioDsp.Speech_enhancement_settings"), |ui| {
        ui.label(label("settings.audioDsp.RNNoise_is_lightweight_PureVox_uses_more_CPU_Changes_apply_to_the_next_recording"));
        egui::ComboBox::from_id_salt("audio-dsp-algorithm")
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
            ui.add(egui::Slider::new(&mut config.noise_strength, 0..=100).text(label("settings.audioDsp.Noise_reduction")));
        }
        ui.add(egui::Slider::new(&mut config.gain_db, -30..=30).text(label("settings.audioDsp.Input_gain_dB")));
        ui.checkbox(&mut config.agc_enabled, label("settings.audioDsp.Automatic_gain"));
        if config.agc_enabled {
            ui.add(egui::Slider::new(&mut config.agc_target, 5..=90).text(label("settings.audioDsp.Target_level")));
            ui.add(egui::Slider::new(&mut config.agc_attack, 1..=100).text(label("settings.audioDsp.Attack_speed")));
            ui.add(egui::Slider::new(&mut config.agc_decay, 1..=100).text(label("settings.audioDsp.Release_speed")));
        }
        ui.checkbox(&mut config.vad_enabled, label("settings.audioDsp.Speech_gate_does_not_stop_recording"));
        if config.vad_enabled {
            ui.add(egui::Slider::new(&mut config.vad_threshold_db, -80..=-10).text(label("settings.audioDsp.Threshold_dB")));
            ui.small(label("settings.audioDsp.A_high_threshold_can_remove_quiet_speech"));
        }
        ui.collapsing(label("settings.audioDsp.Tone_and_room_adjustments"), |ui| {
            ui.small(label("settings.audioDsp.The_MicYou_comb_filter_can_color_speech_Start_at_0"));
            ui.add(egui::Slider::new(&mut config.dereverb_level, 0..=100).text(label("settings.audioDsp.Comb_filter")));
            ui.checkbox(&mut config.equalizer_enabled, label("settings.audioDsp.10_band_equalizer"));
            if config.equalizer_enabled {
                ui.add(egui::Slider::new(&mut config.equalizer_preamp_db, -20..=20).text(label("settings.audioDsp.Preamp_dB")));
                for (band, name) in config.equalizer_bands_db.iter_mut().zip(["31 Hz", "63 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"]) {
                    ui.add(egui::Slider::new(band, -12..=12).text(name));
                }
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
                    ui.label(name);
                    if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() { swap = Some((i, i - 1)); }
                    if ui.add_enabled(i < 5, egui::Button::new("↓")).clicked() { swap = Some((i, i + 1)); }
                }));
            }
            if let Some((a, b)) = swap { config.processing_order.swap(a, b); }
        });
        if ui.button(label("settings.audioDsp.Reset_enhancement_settings")).clicked() { *config = Default::default(); }
    });
    before != *config
}
