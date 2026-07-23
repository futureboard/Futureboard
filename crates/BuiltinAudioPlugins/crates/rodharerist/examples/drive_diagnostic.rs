//! Offline drive-model diagnostic — NOT part of the shipping plugin.
//!
//! Renders test signals through each dedicated-topology drive model and
//! prints peak / RMS / crest factor / DC offset / harmonic residual, plus a
//! pairwise distinction matrix. Run with:
//!
//! ```text
//! cargo run -p rodharerist --example drive_diagnostic --release
//! ```

use builtin_dsp_core::StereoEffect;
use rodharerist::{DriveModel, Dsp, PATH_SLOTS, StageKind, default_params};

const SR: f32 = 48_000.0;
const N: usize = 48_000;
const SETTLE: usize = 8_000;

fn drive_dsp(model: DriveModel, gain: f32) -> Dsp {
    let mut dsp = Dsp::new(SR);
    let mut p = default_params();
    p.stage_order = [None; PATH_SLOTS];
    p.stage_order[0] = Some(StageKind::Drive);
    p.drive_model = model;
    p.drive_gain = gain;
    dsp.set_params(p);
    dsp
}

fn lcg(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
}

fn render(model: DriveModel, gain: f32, signal: &dyn Fn(usize) -> f32) -> Vec<f32> {
    let mut dsp = drive_dsp(model, gain);
    (0..N)
        .map(|n| {
            let x = signal(n);
            dsp.process_stereo(x, x).0
        })
        .skip(SETTLE)
        .collect()
}

struct Stats {
    peak: f32,
    rms: f32,
    crest_db: f32,
    dc: f32,
}

fn stats(buf: &[f32]) -> Stats {
    let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt();
    let dc = buf.iter().sum::<f32>() / buf.len() as f32;
    Stats {
        peak,
        rms,
        crest_db: 20.0 * (peak / rms.max(1.0e-9)).log10(),
        dc,
    }
}

/// Energy fraction NOT explained by a pure rescale of the input — a cheap
/// "how much waveshaping actually happened" number.
fn harmonic_residual(input: &[f32], output: &[f32]) -> f32 {
    let dot: f32 = input.iter().zip(output).map(|(a, b)| a * b).sum();
    let in_e: f32 = input.iter().map(|a| a * a).sum();
    let scale = dot / in_e.max(1.0e-9);
    let resid: f32 = input
        .iter()
        .zip(output)
        .map(|(a, b)| (b - a * scale).powi(2))
        .sum();
    let out_e: f32 = output.iter().map(|b| b * b).sum();
    resid / out_e.max(1.0e-9)
}

fn main() {
    let models = [
        DriveModel::DsOne,
        DriveModel::SuperDrive,
        DriveModel::MetalCore,
        DriveModel::TightRift,
    ];
    let sines = [100.0f32, 440.0, 1_000.0];

    println!("== per-model signal stats (drive=8, level=default) ==");
    for model in models {
        println!("-- {model:?}");
        for &f in &sines {
            let sig = move |n: usize| (n as f32 * 2.0 * std::f32::consts::PI * f / SR).sin() * 0.5;
            let input: Vec<f32> = (0..N).map(sig).skip(SETTLE).collect();
            let out = render(model, 8.0, &sig);
            let s = stats(&out);
            println!(
                "   sine {f:>6.0} Hz  peak={:>6.3}  rms={:>6.3}  crest={:>5.1} dB  dc={:>+8.5}  harm_resid={:>5.1}%",
                s.peak,
                s.rms,
                s.crest_db,
                s.dc,
                harmonic_residual(&input, &out) * 100.0
            );
        }
        // Impulse train (transient behavior) and low-level noise.
        let imp = |n: usize| if n % 4_800 == 0 { 0.9 } else { 0.0 };
        let s = stats(&render(model, 8.0, &imp));
        println!(
            "   impulses        peak={:>6.3}  rms={:>6.3}  crest={:>5.1} dB  dc={:>+8.5}",
            s.peak, s.rms, s.crest_db, s.dc
        );
        let noise_sig = |n: usize| {
            let mut st = 0x600D_F00Du32.wrapping_add(n as u32 * 2_654_435_761);
            lcg(&mut st) * 0.1
        };
        let s = stats(&render(model, 8.0, &noise_sig));
        println!(
            "   noise (-20 dB)  peak={:>6.3}  rms={:>6.3}  crest={:>5.1} dB  dc={:>+8.5}",
            s.peak, s.rms, s.crest_db, s.dc
        );
    }

    println!("\n== pairwise distinction (rms of output difference, 440 Hz) ==");
    let sig = |n: usize| (n as f32 * 2.0 * std::f32::consts::PI * 440.0 / SR).sin() * 0.5;
    let outs: Vec<Vec<f32>> = models.iter().map(|&m| render(m, 7.0, &sig)).collect();
    for i in 0..models.len() {
        for j in (i + 1)..models.len() {
            let diff = (outs[i]
                .iter()
                .zip(&outs[j])
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                / outs[i].len() as f32)
                .sqrt();
            println!("   {:?} vs {:?}: {:.4}", models[i], models[j], diff);
        }
    }
}
