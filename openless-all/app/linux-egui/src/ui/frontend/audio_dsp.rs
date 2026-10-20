use eframe::egui;
use openless_core::{AudioDspConfig, AudioDspStage, NoiseReduction};
use openless_linux_egui::Lang;

pub fn settings(ui: &mut egui::Ui, config: &mut AudioDspConfig, lang: Lang) -> bool {
    let before = *config;
    let label = |zh, en| {
        if matches!(lang, Lang::ZhCn | Lang::ZhTw) {
            zh
        } else {
            en
        }
    };
    ui.collapsing(label("语音增强设置", "Speech enhancement settings"), |ui| {
        ui.label(label("RNNoise 更轻量，PureVox 模型降噪耗时较高。设置在下一次录音生效。", "RNNoise is lightweight; PureVox uses more CPU. Changes apply to the next recording."));
        egui::ComboBox::from_id_salt("audio-dsp-algorithm")
            .selected_text(match config.noise_reduction {
                NoiseReduction::Off => label("关闭降噪", "Noise reduction off"),
                NoiseReduction::Rnnoise => "RNNoise",
                NoiseReduction::Purevox => "PureVox 6",
                NoiseReduction::Spectral => label("频谱减法", "Spectral subtraction"),
            })
            .show_ui(ui, |ui| {
                for (value, name) in [(NoiseReduction::Off, label("关闭降噪", "Off")), (NoiseReduction::Rnnoise, "RNNoise"), (NoiseReduction::Purevox, "PureVox 6"), (NoiseReduction::Spectral, label("频谱减法", "Spectral subtraction"))] {
                    ui.selectable_value(&mut config.noise_reduction, value, name);
                }
            });
        if config.noise_reduction != NoiseReduction::Off {
            ui.add(egui::Slider::new(&mut config.noise_strength, 0..=100).text(label("降噪强度 %", "Noise reduction %")));
        }
        ui.add(egui::Slider::new(&mut config.gain_db, -30..=30).text(label("输入增益 dB", "Input gain dB")));
        ui.checkbox(&mut config.agc_enabled, label("自动增益", "Automatic gain"));
        if config.agc_enabled {
            ui.add(egui::Slider::new(&mut config.agc_target, 5..=90).text(label("目标电平 %", "Target level %")));
            ui.add(egui::Slider::new(&mut config.agc_attack, 1..=100).text(label("启动速度", "Attack speed")));
            ui.add(egui::Slider::new(&mut config.agc_decay, 1..=100).text(label("释放速度", "Release speed")));
        }
        ui.checkbox(&mut config.vad_enabled, label("语音门限（不自动停止录音）", "Speech gate (does not stop recording)"));
        if config.vad_enabled {
            ui.add(egui::Slider::new(&mut config.vad_threshold_db, -80..=-10).text(label("门限 dB", "Threshold dB")));
            ui.small(label("门限过高可能丢失轻声。", "A high threshold can remove quiet speech."));
        }
        ui.collapsing(label("音色与房间调整", "Tone and room adjustments"), |ui| {
            ui.small(label("梳状滤波沿用 MicYou 的延迟相减方式，可能改变音色，建议从 0% 开始。", "The MicYou comb filter can color speech. Start at 0%."));
            ui.add(egui::Slider::new(&mut config.dereverb_level, 0..=100).text(label("梳状滤波 %", "Comb filter %")));
            ui.checkbox(&mut config.equalizer_enabled, label("十段均衡器", "10-band equalizer"));
            if config.equalizer_enabled {
                ui.add(egui::Slider::new(&mut config.equalizer_preamp_db, -20..=20).text(label("前置增益 dB", "Preamp dB")));
                for (band, name) in config.equalizer_bands_db.iter_mut().zip(["31 Hz", "63 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"]) {
                    ui.add(egui::Slider::new(band, -12..=12).text(name));
                }
            }
        });
        ui.collapsing(label("处理顺序", "Processing order"), |ui| {
            let mut swap = None;
            for (i, stage) in config.processing_order.iter().enumerate() {
                let name = match stage {
                    AudioDspStage::NoiseReduction => label("降噪", "Noise reduction"),
                    AudioDspStage::Dereverb => label("梳状滤波", "Comb filter"),
                    AudioDspStage::Equalizer => label("均衡器", "Equalizer"),
                    AudioDspStage::Amplifier => label("输入增益", "Input gain"),
                    AudioDspStage::Agc => label("自动增益", "Automatic gain"),
                    AudioDspStage::Vad => label("语音门限", "Speech gate"),
                };
                ui.push_id(i, |ui| ui.horizontal(|ui| {
                    ui.label(name);
                    if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() { swap = Some((i, i - 1)); }
                    if ui.add_enabled(i < 5, egui::Button::new("↓")).clicked() { swap = Some((i, i + 1)); }
                }));
            }
            if let Some((a, b)) = swap { config.processing_order.swap(a, b); }
        });
        if ui.button(label("恢复增强默认设置", "Reset enhancement settings")).clicked() { *config = Default::default(); }
    });
    before != *config
}
