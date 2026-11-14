//! Stateful 16 kHz -> 24 kHz mono PCM16 interpolation (3 output / 2 input samples).
//!
//! Recorder callback boundaries do not reset the interpolation phase. Holding the last
//! source sample provides the next interpolation endpoint; finish emits the exact tail.

#[derive(Default)]
pub(super) struct Resampler {
    previous: Option<i16>,
    source_index: u64,
    next_output: u64,
}

impl Resampler {
    pub(super) fn push(&mut self, pcm: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(pcm.len() * 3 / 2 + 4);
        for pair in pcm.chunks_exact(2) {
            let current = i16::from_le_bytes([pair[0], pair[1]]);
            let Some(previous) = self.previous.replace(current) else {
                output.extend_from_slice(&current.to_le_bytes());
                self.next_output = 1;
                continue;
            };
            self.source_index += 1;
            while self.next_output * 2 <= self.source_index * 3 {
                let fraction = (self.next_output * 2 - (self.source_index - 1) * 3) as i32;
                let sample = ((previous as i32 * (3 - fraction) + current as i32 * fraction) / 3) as i16;
                output.extend_from_slice(&sample.to_le_bytes());
                self.next_output += 1;
            }
        }
        output
    }

    pub(super) fn finish(&mut self) -> Vec<u8> {
        let mut output = Vec::new();
        if let Some(last) = self.previous.take() {
            let expected = ((self.source_index + 1) * 3).div_ceil(2);
            while self.next_output < expected {
                output.extend_from_slice(&last.to_le_bytes());
                self.next_output += 1;
            }
        }
        output
    }
}
