//! Sound: effects decoded into memory once and played any number of times
//! at once (volume, pitch), and one music track streamed from its file
//! with an optional loop. WAV, OGG Vorbis and MP3, through rodio.
//!
//! Without a sound device (headless engines, a machine with no output)
//! everything still loads and every play call is silently skipped.

use std::fs::File;
use std::io::{BufReader, Cursor};

use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::source::Buffered;
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};

use crate::texture::Store;

/// A decoded sound effect
pub struct Sound {
    samples: Buffered<SamplesBuffer>,
    pub channels: u16,
    pub sample_rate: u32,
    /// Length in seconds
    pub duration: f32,
}

impl Sound {
    /// Decode WAV, OGG or MP3 bytes
    pub fn decode(bytes: Vec<u8>) -> Result<Sound, String> {
        let decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let (channels, sample_rate) = (decoder.channels(), decoder.sample_rate());
        let data: Vec<f32> = decoder.collect();
        let frames = data.len() as f32 / channels.max(1) as f32;
        Ok(Sound {
            samples: SamplesBuffer::new(channels, sample_rate, data).buffered(),
            channels,
            sample_rate,
            duration: frames / sample_rate.max(1) as f32,
        })
    }

    pub fn load(path: &str) -> Result<Sound, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path, e))?;
        Sound::decode(bytes).map_err(|e| format!("{}: {}", path, e))
    }
}

pub struct Audio {
    pub sounds: Store<Sound>,
    /// Kept alive while sound plays; None without a device
    _stream: Option<OutputStream>,
    mixer: Option<Mixer>,
    /// Effects still playing, by voice id
    voices: Vec<(u32, Sink)>,
    next_voice: u32,
    music: Option<Sink>,
    music_volume: f32,
    master: f32,
}

impl Audio {
    /// No device: sounds load, nothing is heard
    pub fn silent() -> Audio {
        Audio {
            sounds: Store::default(),
            _stream: None,
            mixer: None,
            voices: Vec::new(),
            next_voice: 1,
            music: None,
            music_volume: 1.0,
            master: 1.0,
        }
    }

    /// The default output device, or silent when there is none
    pub fn open() -> Result<Audio, String> {
        let mut stream = OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?;
        stream.log_on_drop(false);
        Ok(Audio::with_mixer(stream.mixer().clone(), Some(stream)))
    }

    /// Mix into a given mixer (tests and offline rendering read its output)
    pub fn with_mixer(mixer: Mixer, stream: Option<OutputStream>) -> Audio {
        Audio {
            _stream: stream,
            mixer: Some(mixer),
            ..Audio::silent()
        }
    }

    pub fn has_device(&self) -> bool {
        self.mixer.is_some()
    }

    /// Drop finished effects (the engine calls this every frame)
    pub fn update(&mut self) {
        self.voices.retain(|(_, sink)| !sink.empty());
    }

    pub fn load_sound(&mut self, path: &str) -> Result<u32, String> {
        Ok(self.sounds.insert(Sound::load(path)?))
    }

    pub fn sound_from_bytes(&mut self, bytes: Vec<u8>) -> Result<u32, String> {
        Ok(self.sounds.insert(Sound::decode(bytes)?))
    }

    pub fn free_sound(&mut self, sound: u32) {
        self.sounds.remove(sound);
    }

    /// Play an effect; `pitch` 1 is the original, 2 an octave up (and
    /// twice as fast). Returns a voice id for `stop_voice`, 0 when
    /// nothing plays.
    pub fn play_sound(&mut self, sound: u32, volume: f32, pitch: f32) -> u32 {
        let (Some(mixer), Some(sound)) = (self.mixer.as_ref(), self.sounds.get(sound)) else {
            return 0;
        };
        // Volume and pitch go on the source itself: sink controls apply
        // only after the first few milliseconds
        let source = sound.samples.clone().amplify(volume.max(0.0) * self.master);
        let sink = Sink::connect_new(mixer);
        sink.append(source.speed(pitch.max(0.01)));
        let id = self.next_voice;
        self.next_voice = self.next_voice.wrapping_add(1).max(1);
        self.voices.push((id, sink));
        id
    }

    pub fn voice_playing(&self, voice: u32) -> bool {
        self.voices.iter().any(|(id, s)| *id == voice && !s.empty())
    }

    pub fn stop_voice(&mut self, voice: u32) {
        self.voices.retain(|(id, sink)| {
            if *id == voice {
                sink.stop();
            }
            *id != voice
        });
    }

    pub fn stop_all_sounds(&mut self) {
        for (_, sink) in self.voices.drain(..) {
            sink.stop();
        }
    }

    /// Stream a music file, replacing the current track
    pub fn play_music(&mut self, path: &str, volume: f32, looped: bool) -> Result<(), String> {
        self.stop_music();
        let open = || {
            File::open(path)
                .map(BufReader::new)
                .map_err(|e| format!("{}: {}", path, e))
        };
        let err = |e: rodio::decoder::DecoderError| format!("{}: {}", path, e);
        // Decode first so a bad file is reported even without a device
        let sink = self.mixer.as_ref().map(Sink::connect_new);
        if looped {
            let source = Decoder::new_looped(open()?).map_err(err)?;
            if let Some(sink) = &sink {
                sink.append(source);
            }
        } else {
            let source = Decoder::new(open()?).map_err(err)?;
            if let Some(sink) = &sink {
                sink.append(source);
            }
        }
        self.music_volume = volume.max(0.0);
        if let Some(sink) = &sink {
            sink.set_volume(self.music_volume * self.master);
        }
        self.music = sink;
        Ok(())
    }

    pub fn stop_music(&mut self) {
        if let Some(sink) = self.music.take() {
            sink.stop();
        }
    }

    pub fn pause_music(&mut self, paused: bool) {
        if let Some(sink) = &self.music {
            if paused {
                sink.pause();
            } else {
                sink.play();
            }
        }
    }

    pub fn music_playing(&self) -> bool {
        self.music
            .as_ref()
            .is_some_and(|s| !s.empty() && !s.is_paused())
    }

    pub fn set_music_volume(&mut self, volume: f32) {
        self.music_volume = volume.max(0.0);
        if let Some(sink) = &self.music {
            sink.set_volume(self.music_volume * self.master);
        }
    }

    /// Scales every effect started after the call, and the music now
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master = volume.max(0.0);
        self.set_music_volume(self.music_volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mono 16-bit WAV of `n` samples, all `value`
    fn wav(n: usize, rate: u32, value: i16) -> Vec<u8> {
        let data_len = (n * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for _ in 0..n {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    #[test]
    fn decodes_and_skips_play_without_a_device() {
        let mut audio = Audio::silent();
        let id = audio.sound_from_bytes(wav(4000, 8000, 1000)).unwrap();
        let sound = audio.sounds.get(id).unwrap();
        assert_eq!((sound.channels, sound.sample_rate), (1, 8000));
        assert!((sound.duration - 0.5).abs() < 1e-6);
        assert_eq!(audio.play_sound(id, 1.0, 1.0), 0);
        assert!(audio.sound_from_bytes(b"not a sound".to_vec()).is_err());
        assert!(audio.play_music("missing.ogg", 1.0, true).is_err());
    }

    #[test]
    fn volume_pitch_and_stop_reach_the_mix() {
        // A stereo 44.1 kHz mix, as from a real device; a mono 0.1 s
        // sound at half amplitude plays into both channels
        let (mixer, mut out) = rodio::mixer::mixer(2, 44100);
        let mut audio = Audio::with_mixer(mixer, None);
        let id = audio.sound_from_bytes(wav(4410, 44100, 16384)).unwrap();
        let heard = |out: &mut rodio::mixer::MixerSource| {
            let mix: Vec<f32> = out.by_ref().take(20000).collect();
            let first = mix.iter().position(|s| s.abs() > 0.01).unwrap_or(0);
            let last = mix.iter().rposition(|s| s.abs() > 0.01).unwrap_or(0);
            (last + 1 - first, mix[(first + last) / 2])
        };

        let voice = audio.play_sound(id, 0.5, 1.0);
        assert!(voice != 0 && audio.voice_playing(voice));
        let (normal, level) = heard(&mut out);
        assert!((8700..=8900).contains(&normal), "{}", normal);
        assert!((level - 0.25).abs() < 0.01, "{}", level);
        audio.update();
        assert!(!audio.voice_playing(voice));

        // Pitch 2 plays in about half the time (the mixer's resampling
        // adds one block of a few hundred samples)
        audio.play_sound(id, 1.0, 2.0);
        let (fast, _) = heard(&mut out);
        assert!(fast >= normal / 2 && fast < normal * 6 / 10, "{}", fast);

        // A stopped voice goes quiet
        let voice = audio.play_sound(id, 1.0, 1.0);
        let _ = out.by_ref().take(1500).count();
        audio.stop_voice(voice);
        let after = out
            .by_ref()
            .skip(1000)
            .take(5000)
            .filter(|s| s.abs() > 0.01)
            .count();
        assert_eq!(after, 0);
    }

    #[test]
    fn music_streams_loops_and_stops() {
        let path = std::env::temp_dir().join(format!("oversight_music_{}.wav", std::process::id()));
        std::fs::write(&path, wav(441, 44100, 16384)).unwrap();
        let path = path.to_string_lossy().to_string();
        let (mixer, mut out) = rodio::mixer::mixer(1, 44100);
        let mut audio = Audio::with_mixer(mixer, None);
        audio.play_music(&path, 1.0, true).unwrap();
        // A 10 ms track looped is still sounding a second later
        let late = out
            .by_ref()
            .skip(44100)
            .take(441)
            .filter(|s| s.abs() > 0.01)
            .count();
        assert!(late > 300, "{}", late);
        assert!(audio.music_playing());
        audio.pause_music(true);
        assert!(!audio.music_playing());
        audio.stop_music();
        let after = out
            .by_ref()
            .skip(2000)
            .take(2000)
            .filter(|s| s.abs() > 0.01)
            .count();
        assert_eq!(after, 0);
        std::fs::remove_file(&path).ok();
    }
}
