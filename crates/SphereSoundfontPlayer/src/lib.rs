//! RustySynth-backed SoundFont player for Futureboard.
//!
//! Loading a SoundFont is a control/offline operation. Rendering assumes the
//! caller owns the output buffers and keeps filesystem I/O out of the audio path.

use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::ffi::CStr;
use std::fs::File;
use std::io::{Cursor, Read};
use std::os::raw::c_char;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::ptr;
use std::slice;
use std::sync::Arc;

const DEFAULT_SAMPLE_RATE: i32 = 44_100;

#[derive(Debug)]
pub enum SoundfontPlayerError {
    InvalidSampleRate(i32),
    InvalidChannel(u8),
    InvalidNote(u8),
    InvalidVelocity(u8),
    InvalidBank(i32),
    InvalidPatch(i32),
    /// `(bank, patch)` requested but not present in the loaded SoundFont's
    /// preset list — distinct from `InvalidBank`/`InvalidPatch`, which reject
    /// out-of-range values before ever consulting the font.
    PresetNotFound { bank: i32, patch: i32 },
    Io(std::io::Error),
    SoundFont(rustysynth::SoundFontError),
    Synthesizer(rustysynth::SynthesizerError),
    BufferLengthMismatch { left: usize, right: usize },
}

impl std::fmt::Display for SoundfontPlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSampleRate(sample_rate) => {
                write!(f, "invalid sample rate: {sample_rate}")
            }
            Self::InvalidChannel(channel) => write!(f, "invalid MIDI channel: {channel}"),
            Self::InvalidNote(note) => write!(f, "invalid MIDI note: {note}"),
            Self::InvalidVelocity(velocity) => write!(f, "invalid MIDI velocity: {velocity}"),
            Self::InvalidBank(bank) => write!(f, "invalid preset bank: {bank}"),
            Self::InvalidPatch(patch) => write!(f, "invalid preset patch: {patch}"),
            Self::PresetNotFound { bank, patch } => {
                write!(f, "no preset at bank {bank} patch {patch} in this SoundFont")
            }
            Self::Io(error) => write!(f, "SoundFont I/O failed: {error}"),
            Self::SoundFont(error) => write!(f, "SoundFont load failed: {error:?}"),
            Self::Synthesizer(error) => write!(f, "synthesizer init failed: {error:?}"),
            Self::BufferLengthMismatch { left, right } => {
                write!(f, "buffer length mismatch: left={left}, right={right}")
            }
        }
    }
}

impl std::error::Error for SoundfontPlayerError {}

impl From<std::io::Error> for SoundfontPlayerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rustysynth::SoundFontError> for SoundfontPlayerError {
    fn from(value: rustysynth::SoundFontError) -> Self {
        Self::SoundFont(value)
    }
}

impl From<rustysynth::SynthesizerError> for SoundfontPlayerError {
    fn from(value: rustysynth::SynthesizerError) -> Self {
        Self::Synthesizer(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundfontPlayerSettings {
    pub sample_rate: i32,
    pub block_size: usize,
    pub maximum_polyphony: usize,
    pub enable_reverb_and_chorus: bool,
}

impl Default for SoundfontPlayerSettings {
    fn default() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            block_size: 0,
            maximum_polyphony: 0,
            enable_reverb_and_chorus: true,
        }
    }
}

impl SoundfontPlayerSettings {
    fn to_rustysynth(self) -> Result<SynthesizerSettings, SoundfontPlayerError> {
        if self.sample_rate <= 0 {
            return Err(SoundfontPlayerError::InvalidSampleRate(self.sample_rate));
        }

        let mut settings = SynthesizerSettings::new(self.sample_rate);
        if self.block_size > 0 {
            settings.block_size = self.block_size;
        }
        if self.maximum_polyphony > 0 {
            settings.maximum_polyphony = self.maximum_polyphony;
        }
        settings.enable_reverb_and_chorus = self.enable_reverb_and_chorus;
        Ok(settings)
    }
}

pub struct SoundfontPlayer {
    synthesizer: Synthesizer,
}

impl SoundfontPlayer {
    pub fn from_path(
        path: impl AsRef<Path>,
        settings: SoundfontPlayerSettings,
    ) -> Result<Self, SoundfontPlayerError> {
        let mut file = File::open(path)?;
        Self::from_reader(&mut file, settings)
    }

    pub fn from_bytes(
        bytes: &[u8],
        settings: SoundfontPlayerSettings,
    ) -> Result<Self, SoundfontPlayerError> {
        let mut cursor = Cursor::new(bytes);
        Self::from_reader(&mut cursor, settings)
    }

    pub fn from_reader<R: Read>(
        reader: &mut R,
        settings: SoundfontPlayerSettings,
    ) -> Result<Self, SoundfontPlayerError> {
        let sound_font = Arc::new(SoundFont::new(reader)?);
        let synth_settings = settings.to_rustysynth()?;
        let synthesizer = Synthesizer::new(&sound_font, &synth_settings)?;
        Ok(Self { synthesizer })
    }

    pub fn note_on(
        &mut self,
        channel: u8,
        note: u8,
        velocity: u8,
    ) -> Result<(), SoundfontPlayerError> {
        validate_channel(channel)?;
        validate_note(note)?;
        if velocity > 127 {
            return Err(SoundfontPlayerError::InvalidVelocity(velocity));
        }

        self.synthesizer
            .note_on(channel.into(), note.into(), velocity.into());
        Ok(())
    }

    pub fn note_off(&mut self, channel: u8, note: u8) -> Result<(), SoundfontPlayerError> {
        validate_channel(channel)?;
        validate_note(note)?;
        self.synthesizer.note_off(channel.into(), note.into());
        Ok(())
    }

    pub fn all_notes_off(&mut self, immediate: bool) {
        self.synthesizer.note_off_all(immediate);
    }

    pub fn process_midi_message(
        &mut self,
        channel: u8,
        command: u8,
        data1: u8,
        data2: u8,
    ) -> Result<(), SoundfontPlayerError> {
        validate_channel(channel)?;
        self.synthesizer.process_midi_message(
            channel.into(),
            command.into(),
            data1.into(),
            data2.into(),
        );
        Ok(())
    }

    pub fn reset(&mut self) {
        self.synthesizer.reset();
    }

    pub fn set_master_volume(&mut self, value: f32) {
        self.synthesizer.set_master_volume(value.max(0.0));
    }

    pub fn master_volume(&self) -> f32 {
        self.synthesizer.get_master_volume()
    }

    pub fn enable_reverb_and_chorus(&self) -> bool {
        self.synthesizer.get_enable_reverb_and_chorus()
    }

    pub fn render(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), SoundfontPlayerError> {
        if left.len() != right.len() {
            return Err(SoundfontPlayerError::BufferLengthMismatch {
                left: left.len(),
                right: right.len(),
            });
        }
        self.synthesizer.render(left, right);
        Ok(())
    }

    pub fn sample_rate(&self) -> i32 {
        self.synthesizer.get_sample_rate()
    }

    pub fn block_size(&self) -> usize {
        self.synthesizer.get_block_size()
    }

    pub fn maximum_polyphony(&self) -> usize {
        self.synthesizer.get_maximum_polyphony()
    }

    /// The loaded SoundFont's own bank name (from its INFO chunk), e.g.
    /// "General MIDI" — control-side metadata for a UI title, not audio state.
    pub fn bank_name(&self) -> &str {
        self.synthesizer.get_sound_font().get_info().get_bank_name()
    }

    /// Every preset (MIDI bank + patch + display name) in the loaded
    /// SoundFont, sorted by `(bank, patch)`. Control/offline operation —
    /// walks the font's preset table, never touches the render path.
    pub fn list_presets(&self) -> Vec<SoundfontPresetInfo> {
        let mut presets: Vec<SoundfontPresetInfo> = self
            .synthesizer
            .get_sound_font()
            .get_presets()
            .iter()
            .map(|preset| SoundfontPresetInfo {
                bank: preset.get_bank_number(),
                patch: preset.get_patch_number(),
                name: preset.get_name().to_string(),
            })
            .collect();
        presets.sort_by_key(|preset| (preset.bank, preset.patch));
        presets
    }

    /// Selects a preset on `channel` via MIDI Bank Select (CC0 MSB / CC32
    /// LSB) followed by Program Change — the standard way to switch patches
    /// on a General MIDI-style synth, so this also works against any other
    /// host that only understands MIDI. Rejects `(bank, patch)` pairs that
    /// only match on `is_empty` (empty rejects "not present"): the caller
    /// should first check [`Self::list_presets`], but this is control-side
    /// validation, not silent fallback to whatever program change lands on.
    pub fn select_preset(
        &mut self,
        channel: u8,
        bank: i32,
        patch: i32,
    ) -> Result<(), SoundfontPlayerError> {
        validate_channel(channel)?;
        if !(0..=16_383).contains(&bank) {
            return Err(SoundfontPlayerError::InvalidBank(bank));
        }
        if !(0..=127).contains(&patch) {
            return Err(SoundfontPlayerError::InvalidPatch(patch));
        }
        if !self
            .list_presets()
            .iter()
            .any(|preset| preset.bank == bank && preset.patch == patch)
        {
            return Err(SoundfontPlayerError::PresetNotFound { bank, patch });
        }

        let bank_msb = (bank >> 7) & 0x7F;
        let bank_lsb = bank & 0x7F;
        self.synthesizer
            .process_midi_message(channel.into(), 0xB0, 0x00, bank_msb);
        self.synthesizer
            .process_midi_message(channel.into(), 0xB0, 0x20, bank_lsb);
        self.synthesizer
            .process_midi_message(channel.into(), 0xC0, patch, 0);
        Ok(())
    }
}

/// One preset (MIDI bank + patch + display name) from a loaded SoundFont.
/// Plain data — no gpui / rendering dependency, so any UI layer (native GPUI,
/// web, or a future host) can build a preset browser from this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundfontPresetInfo {
    pub bank: i32,
    pub patch: i32,
    pub name: String,
}

fn validate_channel(channel: u8) -> Result<(), SoundfontPlayerError> {
    if channel >= Synthesizer::CHANNEL_COUNT as u8 {
        Err(SoundfontPlayerError::InvalidChannel(channel))
    } else {
        Ok(())
    }
}

fn validate_note(note: u8) -> Result<(), SoundfontPlayerError> {
    if note > 127 {
        Err(SoundfontPlayerError::InvalidNote(note))
    } else {
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SphereSoundfontPlayerConfig {
    pub sample_rate: i32,
    pub block_size: usize,
    pub maximum_polyphony: usize,
    pub enable_reverb_and_chorus: u8,
}

impl Default for SphereSoundfontPlayerConfig {
    fn default() -> Self {
        let settings = SoundfontPlayerSettings::default();
        Self {
            sample_rate: settings.sample_rate,
            block_size: settings.block_size,
            maximum_polyphony: settings.maximum_polyphony,
            enable_reverb_and_chorus: u8::from(settings.enable_reverb_and_chorus),
        }
    }
}

impl SphereSoundfontPlayerConfig {
    fn into_settings(self) -> SoundfontPlayerSettings {
        SoundfontPlayerSettings {
            sample_rate: if self.sample_rate == 0 {
                DEFAULT_SAMPLE_RATE
            } else {
                self.sample_rate
            },
            block_size: self.block_size,
            maximum_polyphony: self.maximum_polyphony,
            enable_reverb_and_chorus: self.enable_reverb_and_chorus != 0,
        }
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphereSoundfontPlayerStatus {
    Ok = 0,
    NullPointer = -1,
    InvalidArgument = -2,
    LoadFailed = -3,
    RenderFailed = -4,
    Panic = -5,
}

fn status_from_error(error: &SoundfontPlayerError) -> SphereSoundfontPlayerStatus {
    match error {
        SoundfontPlayerError::Io(_)
        | SoundfontPlayerError::SoundFont(_)
        | SoundfontPlayerError::Synthesizer(_) => SphereSoundfontPlayerStatus::LoadFailed,
        SoundfontPlayerError::BufferLengthMismatch { .. } => {
            SphereSoundfontPlayerStatus::RenderFailed
        }
        SoundfontPlayerError::InvalidSampleRate(_)
        | SoundfontPlayerError::InvalidChannel(_)
        | SoundfontPlayerError::InvalidNote(_)
        | SoundfontPlayerError::InvalidVelocity(_)
        | SoundfontPlayerError::InvalidBank(_)
        | SoundfontPlayerError::InvalidPatch(_)
        | SoundfontPlayerError::PresetNotFound { .. } => {
            SphereSoundfontPlayerStatus::InvalidArgument
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `path` must point to a valid NUL-terminated string, and `out_player` must be
/// a valid writable pointer. The returned handle must be released with
/// [`sphere_soundfont_player_destroy`].
pub unsafe extern "C" fn sphere_soundfont_player_create_from_path(
    path: *const c_char,
    config: SphereSoundfontPlayerConfig,
    out_player: *mut *mut SoundfontPlayer,
) -> i32 {
    if path.is_null() || out_player.is_null() {
        return SphereSoundfontPlayerStatus::NullPointer as i32;
    }
    unsafe {
        *out_player = ptr::null_mut();
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let path = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        SoundfontPlayer::from_path(path, config.into_settings())
    }));

    match result {
        Ok(Ok(player)) => {
            unsafe {
                *out_player = Box::into_raw(Box::new(player));
            }
            SphereSoundfontPlayerStatus::Ok as i32
        }
        Ok(Err(error)) => status_from_error(&error) as i32,
        Err(_) => SphereSoundfontPlayerStatus::Panic as i32,
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// When `len` is non-zero, `data` must point to `len` readable bytes.
/// `out_player` must be a valid writable pointer. The returned handle must be
/// released with [`sphere_soundfont_player_destroy`].
pub unsafe extern "C" fn sphere_soundfont_player_create_from_memory(
    data: *const u8,
    len: usize,
    config: SphereSoundfontPlayerConfig,
    out_player: *mut *mut SoundfontPlayer,
) -> i32 {
    if out_player.is_null() || (data.is_null() && len > 0) {
        return SphereSoundfontPlayerStatus::NullPointer as i32;
    }
    unsafe {
        *out_player = ptr::null_mut();
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let bytes = if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(data, len) }
        };
        SoundfontPlayer::from_bytes(bytes, config.into_settings())
    }));

    match result {
        Ok(Ok(player)) => {
            unsafe {
                *out_player = Box::into_raw(Box::new(player));
            }
            SphereSoundfontPlayerStatus::Ok as i32
        }
        Ok(Err(error)) => status_from_error(&error) as i32,
        Err(_) => SphereSoundfontPlayerStatus::Panic as i32,
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be null or a handle returned by this crate that has not already
/// been destroyed.
pub unsafe extern "C" fn sphere_soundfont_player_destroy(player: *mut SoundfontPlayer) {
    if !player.is_null() {
        unsafe {
            drop(Box::from_raw(player));
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_note_on(
    player: *mut SoundfontPlayer,
    channel: u8,
    note: u8,
    velocity: u8,
) -> i32 {
    with_player(player, |player| player.note_on(channel, note, velocity))
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_note_off(
    player: *mut SoundfontPlayer,
    channel: u8,
    note: u8,
) -> i32 {
    with_player(player, |player| player.note_off(channel, note))
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_all_notes_off(
    player: *mut SoundfontPlayer,
    immediate: u8,
) -> i32 {
    with_player(player, |player| {
        player.all_notes_off(immediate != 0);
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_process_midi_message(
    player: *mut SoundfontPlayer,
    channel: u8,
    command: u8,
    data1: u8,
    data2: u8,
) -> i32 {
    with_player(player, |player| {
        player.process_midi_message(channel, command, data1, data2)
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_reset(player: *mut SoundfontPlayer) -> i32 {
    with_player(player, |player| {
        player.reset();
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_set_master_volume(
    player: *mut SoundfontPlayer,
    value: f32,
) -> i32 {
    with_player(player, |player| {
        player.set_master_volume(value);
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be a valid, uniquely owned handle returned by this crate. When
/// `frames` is non-zero, `left` and `right` must each point to `frames`
/// writable `f32` samples and must not alias each other.
pub unsafe extern "C" fn sphere_soundfont_player_render(
    player: *mut SoundfontPlayer,
    left: *mut f32,
    right: *mut f32,
    frames: usize,
) -> i32 {
    if player.is_null() {
        return SphereSoundfontPlayerStatus::NullPointer as i32;
    }
    if frames == 0 {
        return SphereSoundfontPlayerStatus::Ok as i32;
    }
    if left.is_null() || right.is_null() {
        return SphereSoundfontPlayerStatus::NullPointer as i32;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let player = unsafe { player.as_mut() }.ok_or(SphereSoundfontPlayerStatus::NullPointer)?;
        let left = unsafe { slice::from_raw_parts_mut(left, frames) };
        let right = unsafe { slice::from_raw_parts_mut(right, frames) };
        player
            .render(left, right)
            .map_err(|error| status_from_error(&error))
    }));

    match result {
        Ok(Ok(())) => SphereSoundfontPlayerStatus::Ok as i32,
        Ok(Err(status)) => status as i32,
        Err(_) => SphereSoundfontPlayerStatus::Panic as i32,
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be null or a valid handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_sample_rate(
    player: *const SoundfontPlayer,
) -> i32 {
    if player.is_null() {
        return 0;
    }
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        unsafe { player.as_ref() }.map_or(0, SoundfontPlayer::sample_rate)
    }));
    result.unwrap_or(0)
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be null or a valid handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_block_size(
    player: *const SoundfontPlayer,
) -> usize {
    if player.is_null() {
        return 0;
    }
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        unsafe { player.as_ref() }.map_or(0, SoundfontPlayer::block_size)
    }));
    result.unwrap_or(0)
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `player` must be null or a valid handle returned by this crate.
pub unsafe extern "C" fn sphere_soundfont_player_maximum_polyphony(
    player: *const SoundfontPlayer,
) -> usize {
    if player.is_null() {
        return 0;
    }
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        unsafe { player.as_ref() }.map_or(0, SoundfontPlayer::maximum_polyphony)
    }));
    result.unwrap_or(0)
}

fn with_player(
    player: *mut SoundfontPlayer,
    f: impl FnOnce(&mut SoundfontPlayer) -> Result<(), SoundfontPlayerError>,
) -> i32 {
    if player.is_null() {
        return SphereSoundfontPlayerStatus::NullPointer as i32;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let player = unsafe { player.as_mut() }.ok_or(SphereSoundfontPlayerStatus::NullPointer)?;
        f(player).map_err(|error| status_from_error(&error))
    }));

    match result {
        Ok(Ok(())) => SphereSoundfontPlayerStatus::Ok as i32,
        Ok(Err(status)) => status as i32,
        Err(_) => SphereSoundfontPlayerStatus::Panic as i32,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_soundfont_player_null() -> *mut SoundfontPlayer {
    ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ffi_config_uses_default_settings() {
        let config = SphereSoundfontPlayerConfig::default();
        let settings = config.into_settings();
        assert_eq!(settings.sample_rate, 44_100);
        assert!(settings.enable_reverb_and_chorus);
    }

    #[test]
    fn zero_sample_rate_ffi_config_uses_default() {
        let config = SphereSoundfontPlayerConfig {
            sample_rate: 0,
            ..SphereSoundfontPlayerConfig::default()
        };
        assert_eq!(config.into_settings().sample_rate, 44_100);
    }

    #[test]
    fn invalid_sample_rate_is_rejected() {
        let err = SoundfontPlayerSettings {
            sample_rate: -1,
            ..SoundfontPlayerSettings::default()
        }
        .to_rustysynth()
        .unwrap_err();
        assert!(matches!(err, SoundfontPlayerError::InvalidSampleRate(-1)));
    }

    #[test]
    fn null_render_handle_is_rejected() {
        let status = unsafe {
            sphere_soundfont_player_render(ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), 16)
        };
        assert_eq!(status, SphereSoundfontPlayerStatus::NullPointer as i32);
    }
}
