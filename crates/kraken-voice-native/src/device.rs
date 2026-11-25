//! Les vrais micro et haut-parleurs, par cpal (ALSA sous Linux, donc aussi PulseAudio et
//! PipeWire par leurs modules ALSA ; CoreAudio sous macOS ; WASAPI sous Windows).
//!
//! Le périphérique a sa fréquence, son nombre de voies et son format d'échantillons (cpal rend `f32`,
//! `i16`, `u16`… selon le système : [`CpalCapture::open`] et [`CpalPlayback::open`] les prennent tous) ; le moteur ne
//! connaît que des trames de 48 kHz mono. [`CpalCapture`] et [`CpalPlayback`] convertissent
//! (moyenne des voies et rééchantillonnage vers 48 kHz à l'entrée ; rééchantillonnage et copie
//! sur toutes les voies à la sortie) et se parlent avec le rappel temps réel de cpal par un
//! tampon circulaire sans verrou.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, I24, Sample, SampleFormat, SizedSample, U24};

use crate::Error;
pub use crate::audio::DeviceInfo;
use crate::audio::{Capture, FRAME, Frame, Playback, SAMPLE_RATE};
use crate::resample::Resampler;
use crate::ring::{RingRx, RingTx, ring};

/// Micros disponibles.
pub fn input_devices() -> Result<Vec<DeviceInfo>, Error> {
    list(true)
}

/// Haut-parleurs (ou casques) disponibles.
pub fn output_devices() -> Result<Vec<DeviceInfo>, Error> {
    list(false)
}

fn name_of(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string()).filter(|n| !n.is_empty())
}

fn id_of(device: &cpal::Device) -> String {
    device.id().map(|id| id.to_string()).unwrap_or_default()
}

fn list(input: bool) -> Result<Vec<DeviceInfo>, Error> {
    let host = cpal::default_host();
    let default = if input { host.default_input_device() } else { host.default_output_device() }.map(|d| id_of(&d));
    let devices: Vec<cpal::Device> = if input {
        host.input_devices().map_err(device_error)?.collect()
    } else {
        host.output_devices().map_err(device_error)?.collect()
    };
    let mut out: Vec<DeviceInfo> = Vec::new();
    for d in &devices {
        let Some(name) = name_of(d) else { continue };
        let id = id_of(d);
        // Un nom qui revient (même description pour plusieurs entrées) : la première suffit.
        if !out.iter().any(|o| o.name == name) {
            out.push(DeviceInfo { is_default: default.as_deref() == Some(id.as_str()), name, id });
        }
    }
    // Le périphérique par défaut en premier, puis l'ordre alphabétique.
    out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

fn device_error(e: cpal::Error) -> Error {
    Error::Device(e.to_string())
}

/// Le périphérique de ce nom (ou de cet identifiant cpal), ou celui par défaut si `name` est absent ou vide.
fn find(input: bool, name: Option<&str>) -> Result<cpal::Device, Error> {
    let host = cpal::default_host();
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        let d = if input { host.default_input_device() } else { host.default_output_device() };
        return d.ok_or_else(|| {
            Error::Device(format!("aucun {} par défaut", if input { "micro" } else { "haut-parleur" }))
        });
    };
    let devices: Vec<cpal::Device> = if input {
        host.input_devices().map_err(device_error)?.collect()
    } else {
        host.output_devices().map_err(device_error)?.collect()
    };
    devices
        .into_iter()
        .find(|d| name_of(d).as_deref() == Some(name) || id_of(d) == name)
        .ok_or_else(|| Error::Device(format!("périphérique introuvable : {name}")))
}

/// État partagé entre le périphérique et son côté moteur.
#[derive(Default)]
struct DeviceState {
    /// Pourquoi le flux s'est arrêté, le cas échéant.
    failure: Mutex<Option<String>>,
    /// Dernière taille de bloc demandée par le périphérique (en trames de sa fréquence).
    block: AtomicUsize,
    /// Fois où le périphérique a manqué de son (sortie) ou débordé (entrée).
    glitches: AtomicUsize,
}

impl DeviceState {
    fn failure(&self) -> Option<String> {
        self.failure.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Rappel d'erreur de cpal : les incidents passagers sont notés, les autres arrêtent le périphérique.
    fn on_error(&self, e: cpal::Error) {
        match e.kind() {
            cpal::ErrorKind::Xrun => {
                self.glitches.fetch_add(1, Ordering::Relaxed);
            }
            cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied => tracing::debug!("audio : {e}"),
            _ => {
                tracing::warn!("audio : {e}");
                *self.failure.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.to_string());
            }
        }
    }
}

/// Du micro (voies entrelacées, fréquence du périphérique) vers des échantillons mono à 48 kHz.
struct InputConverter {
    channels: usize,
    resampler: Resampler,
    mono: Vec<f32>,
    out: Vec<f32>,
}

impl InputConverter {
    fn new(channels: usize, rate: u32) -> Self {
        Self {
            channels: channels.max(1),
            resampler: Resampler::new(rate, SAMPLE_RATE),
            mono: Vec::with_capacity(4096),
            out: Vec::with_capacity(4096),
        }
    }

    /// Moyenne des voies, puis rééchantillonnage. Rend aussi combien de trames le périphérique a données.
    fn convert<T: Copy>(&mut self, data: &[T]) -> (usize, &[f32])
    where
        f32: FromSample<T>,
    {
        self.mono.clear();
        for frame in data.chunks_exact(self.channels) {
            self.mono.push(frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / self.channels as f32);
        }
        self.out.clear();
        self.resampler.process(&self.mono, &mut self.out);
        (self.mono.len(), &self.out)
    }
}

/// Écrit des échantillons mono dans le tampon entrelacé du périphérique (copiés sur toutes les voies) ;
/// ce qui manque est du silence.
fn write_interleaved<T: Sample + FromSample<f32>>(data: &mut [T], channels: usize, mono: &[f32]) {
    for (i, out) in data.chunks_exact_mut(channels.max(1)).enumerate() {
        out.fill(T::from_sample(mono.get(i).copied().unwrap_or(0.0)));
    }
}

/// Trames de 48 kHz à garder d'avance pour le périphérique de sortie : deux, ou un bloc du
/// périphérique de plus d'une trame (s'il vient chercher 50 ms d'un coup, il faut au moins ça en réserve).
fn playback_target(block: usize) -> usize {
    (2 * FRAME).max(block + FRAME)
}

/// Le micro, par cpal.
pub struct CpalCapture {
    rx: RingRx,
    state: Arc<DeviceState>,
    name: String,
    _stream: cpal::Stream,
}

/// Retard accumulé au-delà duquel on jette le plus ancien (moteur bloqué un moment) : 120 ms.
const MAX_BACKLOG: usize = 6 * FRAME;
/// Ce qu'on garde alors : 40 ms.
const KEEP_BACKLOG: usize = 2 * FRAME;

impl CpalCapture {
    /// Ouvre le micro appelé `name` (voir [`input_devices`]), ou celui par défaut.
    pub fn open(name: Option<&str>) -> Result<Self, Error> {
        let device = find(true, name)?;
        let supported = device.default_input_config().map_err(device_error)?;
        let (channels, rate) = (usize::from(supported.channels()), supported.sample_rate());
        if channels == 0 || rate == 0 {
            return Err(Error::Device("configuration du micro invalide".into()));
        }
        let (tx, rx) = ring(SAMPLE_RATE as usize / 2);
        let state = Arc::new(DeviceState::default());
        let config = supported.config();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => input_stream::<f32>(&device, config, channels, rate, tx, &state),
            SampleFormat::F64 => input_stream::<f64>(&device, config, channels, rate, tx, &state),
            SampleFormat::I8 => input_stream::<i8>(&device, config, channels, rate, tx, &state),
            SampleFormat::I16 => input_stream::<i16>(&device, config, channels, rate, tx, &state),
            SampleFormat::I24 => input_stream::<I24>(&device, config, channels, rate, tx, &state),
            SampleFormat::I32 => input_stream::<i32>(&device, config, channels, rate, tx, &state),
            SampleFormat::U8 => input_stream::<u8>(&device, config, channels, rate, tx, &state),
            SampleFormat::U16 => input_stream::<u16>(&device, config, channels, rate, tx, &state),
            SampleFormat::U24 => input_stream::<U24>(&device, config, channels, rate, tx, &state),
            SampleFormat::U32 => input_stream::<u32>(&device, config, channels, rate, tx, &state),
            other => Err(Error::Device(format!("format d'échantillons non pris en charge : {other}"))),
        }?;
        stream.play().map_err(device_error)?;
        Ok(Self { rx, state, name: name_of(&device).unwrap_or_default(), _stream: stream })
    }

    /// Nom du périphérique ouvert.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fois où le périphérique a débordé, depuis l'ouverture.
    pub fn glitches(&self) -> usize {
        self.state.glitches.load(Ordering::Relaxed)
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    mut tx: RingTx,
    state: &Arc<DeviceState>,
) -> Result<cpal::Stream, Error>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let mut converter = InputConverter::new(channels, rate);
    let on_data = {
        let state = state.clone();
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let (block, converted) = converter.convert(data);
            state.block.store(block, Ordering::Relaxed);
            if tx.push(converted) < converted.len() {
                state.glitches.fetch_add(1, Ordering::Relaxed);
            }
        }
    };
    let on_error = {
        let state = state.clone();
        move |e: cpal::Error| state.on_error(e)
    };
    device.build_input_stream(config, on_data, on_error, None).map_err(device_error)
}

impl Capture for CpalCapture {
    fn read_frame(&mut self, frame: &mut Frame) -> bool {
        // Le moteur a pris du retard : on garde le plus récent, pour que la voix ne parte pas en différé.
        let len = self.rx.len();
        if len > MAX_BACKLOG {
            self.rx.discard(len - KEEP_BACKLOG);
        }
        if self.rx.len() < FRAME {
            return false;
        }
        self.rx.pop(frame) == FRAME
    }

    fn failure(&self) -> Option<String> {
        self.state.failure()
    }
}

/// Les haut-parleurs, par cpal.
pub struct CpalPlayback {
    tx: RingTx,
    resampler: Resampler,
    scratch: Vec<f32>,
    /// Fréquence du périphérique.
    rate: u32,
    state: Arc<DeviceState>,
    name: String,
    _stream: cpal::Stream,
}

impl CpalPlayback {
    /// Ouvre les haut-parleurs appelés `name` (voir [`output_devices`]), ou ceux par défaut.
    pub fn open(name: Option<&str>) -> Result<Self, Error> {
        let device = find(false, name)?;
        let supported = device.default_output_config().map_err(device_error)?;
        let (channels, rate) = (usize::from(supported.channels()), supported.sample_rate());
        if channels == 0 || rate == 0 {
            return Err(Error::Device("configuration de la sortie invalide".into()));
        }
        // Une demi-seconde de son au plus en attente.
        let (tx, rx) = ring(rate as usize / 2);
        let state = Arc::new(DeviceState::default());
        let config = supported.config();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => output_stream::<f32>(&device, config, channels, rx, &state),
            SampleFormat::F64 => output_stream::<f64>(&device, config, channels, rx, &state),
            SampleFormat::I8 => output_stream::<i8>(&device, config, channels, rx, &state),
            SampleFormat::I16 => output_stream::<i16>(&device, config, channels, rx, &state),
            SampleFormat::I24 => output_stream::<I24>(&device, config, channels, rx, &state),
            SampleFormat::I32 => output_stream::<i32>(&device, config, channels, rx, &state),
            SampleFormat::U8 => output_stream::<u8>(&device, config, channels, rx, &state),
            SampleFormat::U16 => output_stream::<u16>(&device, config, channels, rx, &state),
            SampleFormat::U24 => output_stream::<U24>(&device, config, channels, rx, &state),
            SampleFormat::U32 => output_stream::<u32>(&device, config, channels, rx, &state),
            other => Err(Error::Device(format!("format d'échantillons non pris en charge : {other}"))),
        }?;
        stream.play().map_err(device_error)?;
        Ok(Self {
            tx,
            resampler: Resampler::new(SAMPLE_RATE, rate),
            scratch: Vec::with_capacity(FRAME * 2),
            rate,
            state,
            name: name_of(&device).unwrap_or_default(),
            _stream: stream,
        })
    }

    /// Nom du périphérique ouvert.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fois où le périphérique a manqué de son, depuis l'ouverture.
    pub fn glitches(&self) -> usize {
        self.state.glitches.load(Ordering::Relaxed)
    }

    /// Convertit une durée de la fréquence du périphérique en échantillons à 48 kHz.
    fn to_engine(&self, device_samples: usize) -> usize {
        (device_samples as u64 * u64::from(SAMPLE_RATE) / u64::from(self.rate)) as usize
    }
}

fn output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mut rx: RingRx,
    state: &Arc<DeviceState>,
) -> Result<cpal::Stream, Error>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let mut mono: Vec<f32> = Vec::with_capacity(8192);
    let on_data = {
        let state = state.clone();
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels;
            state.block.store(frames, Ordering::Relaxed);
            mono.resize(frames, 0.0);
            let got = rx.pop(&mut mono);
            if got < frames {
                state.glitches.fetch_add(1, Ordering::Relaxed);
            }
            write_interleaved(data, channels, &mono[..got]);
        }
    };
    let on_error = {
        let state = state.clone();
        move |e: cpal::Error| state.on_error(e)
    };
    device.build_output_stream(config, on_data, on_error, None).map_err(device_error)
}

impl Playback for CpalPlayback {
    fn needs_frame(&mut self) -> bool {
        let block = self.to_engine(self.state.block.load(Ordering::Relaxed));
        self.to_engine(self.tx.len()) < playback_target(block)
    }

    fn write_frame(&mut self, frame: &Frame) {
        self.scratch.clear();
        self.resampler.process(frame, &mut self.scratch);
        self.tx.push(&self.scratch);
    }

    fn failure(&self) -> Option<String> {
        self.state.failure()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sine, tone_share};

    #[test]
    fn listing_devices_never_panics_even_without_a_sound_card() {
        // Sur une machine sans carte son (la CI), la liste peut être vide ou une erreur : aucun des deux n'est un échec.
        if let Ok(list) = input_devices() {
            assert!(list.iter().filter(|d| d.is_default).count() <= 1);
        }
        let _ = output_devices();
    }

    #[test]
    fn an_unknown_device_is_a_clear_error() {
        let name = "ce périphérique n'existe pas";
        assert!(matches!(CpalCapture::open(Some(name)), Err(Error::Device(_))));
        assert!(matches!(CpalPlayback::open(Some(name)), Err(Error::Device(_))));
    }

    /// Le micro d'un portable : 44,1 kHz, deux voies, entiers 16 bits, par blocs de 441 trames.
    #[test]
    fn microphone_samples_become_mono_48_khz_whatever_the_device() {
        let tone = sine(1000.0, 0.5, 44_100, 44_100.0);
        let to_i16 = |x: f32| (x * f32::from(i16::MAX)) as i16;
        let mut converter = InputConverter::new(2, 44_100);
        let mut out = Vec::new();
        let mut blocks = 0;
        for block in tone.chunks(441) {
            // Voie gauche : la sinusoïde ; voie droite : la même (la moyenne la garde telle quelle).
            let stereo: Vec<i16> = block.iter().flat_map(|x| [to_i16(*x), to_i16(*x)]).collect();
            let (frames, converted) = converter.convert(&stereo);
            assert_eq!(frames, 441);
            out.extend_from_slice(converted);
            blocks += 1;
        }
        assert_eq!(blocks, 100);
        assert!((out.len() as i64 - 48_000).abs() < 100, "{} échantillons pour 1 s", out.len());
        let body = &out[12_000..36_000];
        assert!(tone_share(body, 1000.0) > 0.999);
        let peak = body.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak - 0.5).abs() < 0.01, "crête {peak}");

        // Une seule voie bruyante sur deux : la moyenne divise par deux.
        let mut converter = InputConverter::new(2, 48_000);
        let one_side: Vec<f32> = tone[..960].iter().flat_map(|x| [*x, 0.0]).collect();
        let (_, converted) = converter.convert(&one_side);
        for (c, x) in converted.iter().zip(&tone) {
            assert!((c - x / 2.0).abs() < 1e-6);
        }
    }

    #[test]
    fn a_device_with_other_sample_formats_is_converted_too() {
        let mut converter = InputConverter::new(1, 48_000);
        let (frames, out) = converter.convert(&[128u8, 255, 0, 128]);
        assert_eq!(frames, 4);
        assert!(out[0].abs() < 0.01 && (out[1] - 1.0).abs() < 0.01 && (out[2] + 1.0).abs() < 0.01);
    }

    /// Les formats que cpal rend selon le système (CoreAudio : `f32` ; WASAPI : `f32`, parfois `i16` ou 24 bits ; ALSA : `i16`,
    /// `i32`, 24 bits…) donnent tous le même signal, à l'entrée comme à la sortie.
    #[test]
    fn every_sample_format_the_system_may_use_gives_the_same_signal() {
        let tone = sine(1000.0, 0.5, 4800, 48_000.0);
        fn captured<T: Copy>(tone: &[f32], to: impl Fn(f32) -> T) -> Vec<f32>
        where
            f32: FromSample<T>,
        {
            let data: Vec<T> = tone.iter().flat_map(|x| [to(*x), to(*x)]).collect();
            InputConverter::new(2, 48_000).convert(&data).1.to_vec()
        }
        fn played<T: Sample + FromSample<f32>>(tone: &[f32]) -> Vec<f32>
        where
            f32: FromSample<T>,
        {
            let mut data = vec![T::EQUILIBRIUM; tone.len() * 2];
            write_interleaved(&mut data, 2, tone);
            assert!(data.chunks(2).all(|c| c[0].to_sample::<f32>() == c[1].to_sample::<f32>()), "voies identiques");
            data.chunks(2).map(|c| c[0].to_sample::<f32>()).collect()
        }
        let heard = [
            ("f32", captured(&tone, |x| x)),
            ("f64", captured(&tone, f64::from)),
            ("i8", captured(&tone, i8::from_sample)),
            ("i16", captured(&tone, i16::from_sample)),
            ("i24", captured(&tone, I24::from_sample)),
            ("i32", captured(&tone, i32::from_sample)),
            ("u8", captured(&tone, u8::from_sample)),
            ("u16", captured(&tone, u16::from_sample)),
            ("u24", captured(&tone, U24::from_sample)),
            ("u32", captured(&tone, u32::from_sample)),
        ];
        let spoken = [
            ("f32", played::<f32>(&tone)),
            ("f64", played::<f64>(&tone)),
            ("i8", played::<i8>(&tone)),
            ("i16", played::<i16>(&tone)),
            ("i24", played::<I24>(&tone)),
            ("i32", played::<i32>(&tone)),
            ("u8", played::<u8>(&tone)),
            ("u16", played::<u16>(&tone)),
            ("u24", played::<U24>(&tone)),
            ("u32", played::<u32>(&tone)),
        ];
        for (format, signal) in heard.iter().chain(&spoken) {
            assert_eq!(signal.len(), tone.len(), "{format}");
            let worst = signal.iter().zip(&tone).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst < 0.01, "{format} : écart {worst}");
        }
    }

    /// Des voies et des fréquences que les cartes son rendent (micro d'ordinateur portable, interface USB à 4 entrées, sortie
    /// 7.1 à 96 kHz, 192 kHz) : toujours une seule voix à 48 kHz, au même niveau.
    #[test]
    fn any_channel_count_and_rate_becomes_the_same_mono_signal() {
        for (channels, rate) in
            [(1, 44_100), (1, 48_000), (2, 48_000), (4, 44_100), (6, 48_000), (8, 96_000), (2, 192_000)]
        {
            let tone = sine(1000.0, 0.5, rate as usize, f64::from(rate));
            let data: Vec<f32> = tone.iter().flat_map(|x| vec![*x; channels]).collect();
            let mut converter = InputConverter::new(channels, rate);
            let mut out = Vec::new();
            for block in data.chunks(channels * 480) {
                out.extend_from_slice(converter.convert(block).1);
            }
            assert!(
                (out.len() as i64 - 48_000).abs() < 400,
                "{channels} voies à {rate} Hz : {} échantillons pour 1 s",
                out.len()
            );
            let body = &out[12_000..36_000];
            assert!(tone_share(body, 1000.0) > 0.999, "{channels} voies à {rate} Hz");
            let peak = body.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak - 0.5).abs() < 0.02, "{channels} voies à {rate} Hz : crête {peak}");
        }
    }

    /// Ce que la liste donne se retrouve : par son nom comme par son identifiant (qui change de forme selon le système :
    /// `alsa:…`, un UID CoreAudio, un identifiant de point de terminaison WASAPI). Sans carte son, la liste est vide ou refusée.
    #[test]
    fn a_listed_device_is_found_again_by_name_and_by_id() {
        for input in [true, false] {
            let Ok(listed) = list(input) else { continue };
            assert!(listed.iter().all(|d| !d.name.is_empty() && !d.id.is_empty()), "nom et identifiant non vides");
            for d in &listed {
                let by_name = find(input, Some(&d.name)).expect("trouvé par son nom");
                let by_id = find(input, Some(&d.id)).expect("trouvé par son identifiant");
                assert_eq!(id_of(&by_id), d.id);
                assert_eq!(name_of(&by_name).as_deref(), Some(d.name.as_str()));
            }
        }
    }

    #[test]
    fn playback_samples_fill_every_channel_and_pad_with_silence() {
        let mono = [0.5f32, -0.5, 0.25];
        let mut stereo = [0.0f32; 10];
        write_interleaved(&mut stereo, 2, &mono);
        assert_eq!(stereo, [0.5, 0.5, -0.5, -0.5, 0.25, 0.25, 0.0, 0.0, 0.0, 0.0]);
        // Entiers non signés : le silence est le milieu de l'échelle, pas zéro.
        let mut wide = [0u16; 6];
        write_interleaved(&mut wide, 3, &[1.0]);
        assert_eq!(wide[3..], [32768, 32768, 32768]);
        assert!(wide[0] > 65000 && wide[0] == wide[1] && wide[1] == wide[2]);
        // Un tampon qui n'est pas un nombre entier de trames : le reste est laissé tel quel.
        let mut odd = [7i16; 5];
        write_interleaved(&mut odd, 2, &[0.0, 0.0]);
        assert_eq!(odd[4], 7);
    }

    #[test]
    fn the_output_queue_target_follows_the_device_block() {
        assert_eq!(playback_target(0), 2 * FRAME);
        assert_eq!(playback_target(FRAME / 2), 2 * FRAME);
        assert_eq!(playback_target(FRAME), 2 * FRAME);
        assert_eq!(playback_target(2400), 2400 + FRAME, "un bloc de 50 ms : 50 ms de réserve en plus d'une trame");
    }
}
