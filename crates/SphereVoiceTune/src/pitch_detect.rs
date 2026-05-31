/// Monophonic pitch detection using a simplified YIN algorithm.
/// Returns a tuple of (frequency_hz, confidence).
pub fn detect_pitch_yin(
    frame: &[f32],
    sample_rate: u32,
    min_freq: f64,
    max_freq: f64,
    voiced_threshold: f64,
) -> (f64, f64) {
    if sample_rate == 0 || frame.is_empty() || min_freq <= 0.0 || max_freq <= min_freq {
        return (0.0, 0.0);
    }

    let tau_min = (sample_rate as f64 / max_freq).round() as usize;
    let tau_max = (sample_rate as f64 / min_freq).round() as usize;

    if frame.len() <= tau_max {
        return (0.0, 0.0);
    }

    // Window size for the difference function.
    let w = frame.len() - tau_max;

    // 1. Difference function d(tau)
    let mut d = vec![0.0f64; tau_max];
    for tau in 0..tau_max {
        let mut sum = 0.0;
        for j in 0..w {
            let diff = frame[j] as f64 - frame[j + tau] as f64;
            sum += diff * diff;
        }
        d[tau] = sum;
    }

    // 2. Cumulative mean normalized difference function d'(tau)
    let mut d_prime = vec![0.0f64; tau_max];
    d_prime[0] = 1.0;
    let mut running_sum = 0.0;
    for tau in 1..tau_max {
        running_sum += d[tau];
        if running_sum > 0.0 {
            d_prime[tau] = d[tau] / (running_sum / tau as f64);
        } else {
            d_prime[tau] = 1.0;
        }
    }

    // 3. Absolute thresholding: find first local minimum below threshold
    // Typical YIN threshold is 0.1 to 0.2.
    let threshold = 0.15;
    let mut opt_tau = None;
    for tau in tau_min..tau_max {
        if d_prime[tau] < threshold {
            // Check if it is a local minimum
            if tau + 1 < tau_max
                && d_prime[tau] < d_prime[tau - 1]
                && d_prime[tau] < d_prime[tau + 1]
            {
                opt_tau = Some(tau);
                break;
            }
        }
    }

    // If no tau was found below the threshold, find the global minimum in range
    let tau = match opt_tau {
        Some(t) => t,
        None => {
            let mut min_val = f64::MAX;
            let mut min_tau = 0;
            for (t, &value) in d_prime.iter().enumerate().take(tau_max).skip(tau_min) {
                if value < min_val {
                    min_val = value;
                    min_tau = t;
                }
            }
            min_tau
        }
    };

    if tau == 0 || tau >= tau_max {
        return (0.0, 0.0);
    }

    // 4. Parabolic interpolation for sub-sample accuracy
    let mut tau_interpolated = tau as f64;
    if tau > 0 && tau + 1 < d_prime.len() {
        let alpha = d_prime[tau - 1];
        let beta = d_prime[tau];
        let gamma = d_prime[tau + 1];
        let denom = alpha - 2.0 * beta + gamma;
        if denom.abs() > 1e-5 {
            let p = 0.5 * (alpha - gamma) / denom;
            if p.abs() <= 0.5 {
                tau_interpolated = tau as f64 + p;
            }
        }
    }

    let freq = sample_rate as f64 / tau_interpolated;
    let confidence = 1.0 - d_prime[tau].min(1.0);

    if freq >= min_freq && freq <= max_freq && confidence >= voiced_threshold {
        (freq, confidence)
    } else {
        (0.0, confidence)
    }
}

/// Calculate the root-mean-square (RMS) energy of a frame of samples.
pub fn calculate_rms(frame: &[f32]) -> f64 {
    if frame.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    for &sample in frame {
        sum += sample as f64 * sample as f64;
    }
    (sum / frame.len() as f64).sqrt()
}
