// SPDX-License-Identifier: AGPL-3.0-or-later
//! Convert integer presentation timestamps to clip-relative nanoseconds.
//! This tool intentionally avoids floating-point arithmetic.

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

const MAX_INPUT_BYTES: usize = 65_536;
const MAX_PTS_COUNT: usize = 1_801;
const NS_PER_SECOND: i128 = 1_000_000_000;

struct Config {
    time_base_num: i64,
    time_base_den: i64,
    timestamps_out: String,
    origin_ns_out: String,
    origin_ticks_out: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("pts-to-ns: {error}");
        usage();
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(&env::args().skip(1).collect::<Vec<_>>())?;
    let mut input = String::new();
    io::stdin()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_string(&mut input)
        .map_err(|error| format!("cannot read PTS input: {error}"))?;
    if input.len() > MAX_INPUT_BYTES {
        return Err("PTS input exceeds its byte bound".to_string());
    }

    let (origin_ticks, origin_ns, timestamps_ns) =
        normalize_pts(&input, config.time_base_num, config.time_base_den)?;
    write_new(&config.timestamps_out, &format_lines(&timestamps_ns))?;
    write_new(&config.origin_ns_out, &format!("{origin_ns}\n"))?;
    write_new(&config.origin_ticks_out, &format!("{origin_ticks}\n"))?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut time_base: Option<&str> = None;
    let mut timestamps_out: Option<String> = None;
    let mut origin_ns_out: Option<String> = None;
    let mut origin_ticks_out: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        index += 1;
        let value = args.get(index).ok_or_else(|| format!("{option} requires a value"))?;
        match option {
            "--time-base" if time_base.is_none() => time_base = Some(value),
            "--timestamps-out" if timestamps_out.is_none() => timestamps_out = Some(value.clone()),
            "--origin-ns-out" if origin_ns_out.is_none() => origin_ns_out = Some(value.clone()),
            "--origin-ticks-out" if origin_ticks_out.is_none() => origin_ticks_out = Some(value.clone()),
            "--time-base" | "--timestamps-out" | "--origin-ns-out" | "--origin-ticks-out" => {
                return Err(format!("{option} may be supplied only once"));
            }
            _ => return Err(format!("unknown option: {option}")),
        }
        index += 1;
    }

    let time_base = time_base.ok_or("--time-base is required")?;
    let (num, den) = time_base.split_once('/').ok_or("--time-base must be NUM/DEN")?;
    if den.contains('/') {
        return Err("--time-base must be NUM/DEN".to_string());
    }
    let time_base_num = num.parse::<i64>().map_err(|_| "invalid time-base numerator")?;
    let time_base_den = den.parse::<i64>().map_err(|_| "invalid time-base denominator")?;
    if time_base_num <= 0 || time_base_den <= 0 {
        return Err("time-base numerator and denominator must be positive".to_string());
    }

    Ok(Config {
        time_base_num,
        time_base_den,
        timestamps_out: timestamps_out.ok_or("--timestamps-out is required")?,
        origin_ns_out: origin_ns_out.ok_or("--origin-ns-out is required")?,
        origin_ticks_out: origin_ticks_out.ok_or("--origin-ticks-out is required")?,
    })
}

fn normalize_pts(input: &str, num: i64, den: i64) -> Result<(i64, i128, Vec<u64>), String> {
    if num <= 0 || den <= 0 {
        return Err("time-base numerator and denominator must be positive".to_string());
    }
    if input.len() > MAX_INPUT_BYTES {
        return Err("PTS input exceeds its byte bound".to_string());
    }

    let mut pts = Vec::new();
    for (line_index, line) in input.lines().enumerate() {
        if line_index >= MAX_PTS_COUNT {
            return Err(format!("PTS count exceeds {MAX_PTS_COUNT}"));
        }
        let line = line.trim();
        if line.is_empty() {
            return Err(format!("blank PTS at line {}", line_index + 1));
        }
        pts.push(line.parse::<i64>().map_err(|_| format!("invalid signed integer PTS at line {}", line_index + 1))?);
    }
    if !(3..=MAX_PTS_COUNT).contains(&pts.len()) {
        return Err(format!("provide 3..={MAX_PTS_COUNT} presentation timestamps"));
    }
    for (index, pair) in pts.windows(2).enumerate() {
        if pair[1] <= pair[0] {
            return Err(format!("PTS must be strictly increasing at indexes {index} and {}", index + 1));
        }
    }

    let origin_ticks = pts[0];
    let origin_ns = ticks_to_ns(i128::from(origin_ticks), num, den)?;
    let mut timestamps_ns = Vec::with_capacity(pts.len());
    let mut previous_ns: Option<u64> = None;
    for (index, tick) in pts.iter().copied().enumerate() {
        let delta_ticks = i128::from(tick) - i128::from(origin_ticks);
        let ns = ticks_to_ns(delta_ticks, num, den)?;
        if ns < 0 {
            return Err(format!("relative PTS became negative at index {index}"));
        }
        let ns = u64::try_from(ns).map_err(|_| format!("relative PTS exceeds u64 nanoseconds at index {index}"))?;
        if previous_ns.is_some_and(|previous| ns <= previous) {
            return Err(format!("distinct PTS values collapse to non-increasing integer nanoseconds at index {index}"));
        }
        timestamps_ns.push(ns);
        previous_ns = Some(ns);
    }
    if timestamps_ns.first() != Some(&0) {
        return Err("relative timestamp origin must normalize to zero".to_string());
    }
    Ok((origin_ticks, origin_ns, timestamps_ns))
}

fn ticks_to_ns(ticks: i128, num: i64, den: i64) -> Result<i128, String> {
    ticks
        .checked_mul(i128::from(num))
        .and_then(|value| value.checked_mul(NS_PER_SECOND))
        .map(|value| value / i128::from(den))
        .ok_or_else(|| "PTS-to-nanoseconds conversion overflowed checked i128 arithmetic".to_string())
}

fn format_lines(timestamps: &[u64]) -> String {
    let mut result = String::with_capacity(timestamps.len() * 22);
    for timestamp in timestamps {
        result.push_str(&timestamp.to_string());
        result.push('\n');
    }
    result
}

fn write_new(path: &str, contents: &str) -> Result<(), String> {
    let path = Path::new(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create output '{}': {error}", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("cannot write output '{}': {error}", path.display()))
}

fn usage() {
    eprintln!(
        "Usage: pts-to-ns --time-base NUM/DEN --timestamps-out FILE \\
         --origin-ns-out FILE --origin-ticks-out FILE < pts-per-line.txt"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_90khz_time_base_using_integer_math() {
        let (origin_ticks, origin_ns, timestamps) =
            normalize_pts("90000\n93000\n96000\n", 1, 90_000).unwrap();
        assert_eq!(origin_ticks, 90_000);
        assert_eq!(origin_ns, 1_000_000_000);
        assert_eq!(timestamps, vec![0, 33_333_333, 66_666_666]);
    }

    #[test]
    fn preserves_one_tick_steps_above_f64_exact_integer_range() {
        let (origin_ticks, origin_ns, timestamps) = normalize_pts(
            "9007199254740993\n9007199254740994\n9007199254740995\n",
            1,
            1_000_000_000,
        )
        .unwrap();
        assert_eq!(origin_ticks, 9_007_199_254_740_993);
        assert_eq!(origin_ns, 9_007_199_254_740_993);
        assert_eq!(timestamps, vec![0, 1, 2]);
    }

    #[test]
    fn rejects_non_monotonic_pts() {
        assert!(normalize_pts("100\n200\n199\n", 1, 1_000).unwrap_err().contains("strictly increasing"));
    }

    #[test]
    fn rejects_timestamps_that_collapse_below_nanosecond_resolution() {
        assert!(normalize_pts("1\n2\n3\n", 1, 2_000_000_000).unwrap_err().contains("collapse"));
    }

    #[test]
    fn rejects_checked_integer_overflow() {
        assert!(normalize_pts(
            "9223372036854775807\n9223372036854775806\n9223372036854775805\n",
            i64::MAX,
            1,
        )
        .is_err());
    }

    #[test]
    fn rejects_malformed_or_incomplete_pts_lists() {
        assert!(normalize_pts("1\nx\n3\n", 1, 1).is_err());
        assert!(normalize_pts("1\n2\n", 1, 1).is_err());
    }
}
