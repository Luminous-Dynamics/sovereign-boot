//! Reproducible CPU qualification harness for the Ouroboros semantic runtime.
//!
//! Run in release mode:
//!   cargo run --release -p sovereign-visual-core --example ouroboros-benchmark -- --iterations 250 --warmup 20
//!
//! The JSON output reports p50/p95/p99 and the source/profile metadata that can
//! be recorded without querying a display or GPU. Set BENCH_SOURCE_REV and
//! BENCH_CPU_MODEL in the environment when collecting a named baseline.
//! This measures CPU reference operations only; it does not qualify GPU,
//! compositor presentation, power draw, or physical display timing.

use std::{
    env,
    hint::black_box,
    process,
    time::{Duration, Instant},
};

use sovereign_visual_core::{
    ouroboros::{
        AttentionTarget, OuroborosConfig, OuroborosForm, OuroborosMotionMode, OuroborosScene,
    },
    ouroboros_evidence::capture_ouroboros_evidence,
    ouroboros_mesh::{OuroborosMesh, OuroborosMeshBudget, OuroborosMeshConfig, OuroborosMeshPlan},
    ouroboros_render::render_reference_artifact,
    ouroboros_trace::{OuroborosEvent, OuroborosTrace},
};

#[derive(Debug, Clone, Copy)]
struct Options {
    iterations: usize,
    warmup: usize,
}

#[derive(Debug)]
struct Measurement {
    name: &'static str,
    samples_ns: Vec<u64>,
    sink: u64,
}

impl Measurement {
    fn new(name: &'static str, capacity: usize) -> Self {
        Self {
            name,
            samples_ns: Vec::with_capacity(capacity),
            sink: 0,
        }
    }

    fn as_json(&mut self, warmup: usize, iterations: usize) -> String {
        self.samples_ns.sort_unstable();
        let percentile = |p: f64| -> u64 {
            let index = ((p * self.samples_ns.len() as f64).ceil() as usize)
                .saturating_sub(1)
                .min(self.samples_ns.len().saturating_sub(1));
            self.samples_ns[index]
        };
        let total: u128 = self.samples_ns.iter().map(|&value| u128::from(value)).sum();
        let mean = total as f64 / self.samples_ns.len() as f64;
        let p50 = percentile(0.50);
        let p95 = percentile(0.95);
        let p99 = percentile(0.99);
        let min = *self.samples_ns.first().unwrap_or(&0);
        let max = *self.samples_ns.last().unwrap_or(&0);
        let throughput = if mean > 0.0 {
            1_000_000_000.0 / mean
        } else {
            0.0
        };

        format!(
            concat!(
                "{{\"type\":\"measurement\",\"name\":\"{}\",",
                "\"warmup\":{},\"iterations\":{},",
                "\"min_ns\":{},\"mean_ns\":{:.2},\"p50_ns\":{},",
                "\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},",
                "\"throughput_ops_s\":{:.2},\"sink\":{}}}"
            ),
            self.name, warmup, iterations, min, mean, p50, p95, p99, max, throughput, self.sink
        )
    }
}

fn parse_options() -> Options {
    let mut options = Options {
        iterations: 250,
        warmup: 20,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iterations" => {
                let Some(value) = args.next().and_then(|value| value.parse::<usize>().ok()) else {
                    eprintln!("--iterations requires a positive integer");
                    process::exit(2);
                };
                options.iterations = value;
            }
            "--warmup" => {
                let Some(value) = args.next().and_then(|value| value.parse::<usize>().ok()) else {
                    eprintln!("--warmup requires a non-negative integer");
                    process::exit(2);
                };
                options.warmup = value;
            }
            "--help" | "-h" => {
                println!(
                    "ouroboros-benchmark [--iterations N] [--warmup N]\n\
                     Defaults: --iterations 250 --warmup 20"
                );
                process::exit(0);
            }
            other => {
                eprintln!("unknown argument: {other}");
                process::exit(2);
            }
        }
    }
    if options.iterations == 0 || options.iterations > 100_000 {
        eprintln!("--iterations must be in 1..=100000");
        process::exit(2);
    }
    if options.warmup > 100_000 {
        eprintln!("--warmup must be <=100000");
        process::exit(2);
    }
    options
}

fn measure<F>(name: &'static str, options: Options, mut operation: F) -> Measurement
where
    F: FnMut() -> u64,
{
    let mut measurement = Measurement::new(name, options.iterations);
    for _ in 0..options.warmup {
        measurement.sink ^= black_box(operation());
    }
    for _ in 0..options.iterations {
        let start = Instant::now();
        let value = black_box(operation());
        let elapsed = start.elapsed();
        measurement.samples_ns.push(duration_ns(elapsed));
        measurement.sink = measurement.sink.rotate_left(7) ^ value;
    }
    measurement
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value.is_control() => escaped.push('?'),
            value => escaped.push(value),
        }
    }
    escaped
}

fn make_scene() -> OuroborosScene {
    OuroborosScene::new(OuroborosConfig::default(), OuroborosForm::Nature)
        .expect("default Ouroboros configuration must validate")
}

fn make_trace() -> OuroborosTrace {
    let mut trace = OuroborosTrace::new(OuroborosConfig::default(), OuroborosForm::Nature);
    trace
        .push(OuroborosEvent::TransitionTo {
            form: OuroborosForm::Digital,
        })
        .expect("transition event is valid");
    trace
        .push(OuroborosEvent::SetAttentionTarget {
            target: AttentionTarget::new(0.55, -0.25).expect("normalized target is valid"),
        })
        .expect("attention event is valid");
    for _ in 0..48 {
        trace
            .push(OuroborosEvent::Advance {
                dt_seconds: 1.0 / 60.0,
            })
            .expect("bounded step is valid");
    }
    trace
        .push(OuroborosEvent::SetMotionMode {
            mode: OuroborosMotionMode::Balanced,
        })
        .expect("motion mode event is valid");
    trace
}

fn make_plan(scene: &OuroborosScene) -> OuroborosMeshPlan {
    OuroborosMeshConfig::plan_for_budget(
        scene,
        0.025,
        OuroborosMeshBudget {
            projected_major_radius_px: 160,
            projected_tube_radius_px: 8,
            max_vertices: 40_000,
            max_indices: 240_000,
            max_buffer_bytes: 2_000_000,
        },
    )
    .expect("benchmark mesh budget must fit at least one quality tier")
}

fn run_benchmarks(options: Options) {
    let trace = make_trace();
    let trace_bytes = trace.encode().expect("benchmark trace must encode");
    let replayed_scene = trace.replay().expect("benchmark trace must replay").scene;
    let plan = make_plan(&replayed_scene);
    let base_scene = make_scene();
    let mut step_scene = make_scene();
    step_scene.transition_to(OuroborosForm::Digital);
    let mut retarget_scene = make_scene();
    let mut retarget_steps = 0_u32;

    let mut measurements = Vec::new();

    measurements.push(measure("loop_control", options, || 1));

    measurements.push(measure("scene_advance_60hz", options, || {
        step_scene
            .advance(1.0 / 60.0)
            .expect("60 Hz step is valid");
        u64::from(step_scene.frame().transition_progress.to_bits())
    }));

    // A separate adversarial/interaction workload retargets once per second
    // while the scene continues to step at 60 Hz. This prevents transition
    // command cost from being conflated with the ordinary frame-step cost.
    measurements.push(measure("scene_retarget_1hz_at_60fps", options, || {
        if retarget_steps.is_multiple_of(60) {
            let target = if retarget_scene.frame().target_form == OuroborosForm::Nature {
                OuroborosForm::Digital
            } else {
                OuroborosForm::Nature
            };
            retarget_scene.transition_to(target);
        }
        retarget_scene
            .advance(1.0 / 60.0)
            .expect("60 Hz retarget stress step is valid");
        retarget_steps = retarget_steps.wrapping_add(1);
        u64::from(retarget_scene.frame().transition_progress.to_bits())
    }));

    measurements.push(measure("semantic_landmarks", options, || {
        let receipt = base_scene.landmarks().receipt();
        u64::from_le_bytes(
            receipt.digest[..8]
                .try_into()
                .expect("digest prefix has 8 bytes"),
        )
    }));

    measurements.push(measure("checkpoint_encode_decode", options, || {
        let (bytes, receipt) = base_scene
            .encode_checkpoint()
            .expect("scene checkpoint must encode");
        let (restored, restored_receipt) =
            OuroborosScene::restore_checkpoint(&bytes).expect("scene checkpoint must restore");
        assert_eq!(receipt, restored_receipt);
        black_box(restored.frame());
        u64::from_le_bytes(
            receipt.digest[..8]
                .try_into()
                .expect("digest prefix has 8 bytes"),
        )
    }));

    measurements.push(measure("trace_decode_replay", options, || {
        let decoded = OuroborosTrace::decode(&trace_bytes).expect("trace must decode");
        let replay = decoded.replay().expect("decoded trace must replay");
        u64::from_le_bytes(
            replay.receipt.final_state_digest[..8]
                .try_into()
                .expect("digest prefix has 8 bytes"),
        )
    }));

    measurements.push(measure("mesh_lod_planning", options, || {
        let result = make_plan(&replayed_scene);
        u64::from(result.vertex_count) ^ u64::from(result.estimated_buffer_bytes)
    }));

    measurements.push(measure("mesh_build_validate", options, || {
        let (mesh, receipt) =
            OuroborosMesh::build(&replayed_scene, plan.config).expect("mesh must build");
        mesh.validate().expect("mesh must validate");
        u64::from_le_bytes(
            receipt.digest[..8]
                .try_into()
                .expect("digest prefix has 8 bytes"),
        )
    }));

    measurements.push(measure("cpu_reference_capture_320", options, || {
        let (pixels, receipt) = render_reference_artifact(&replayed_scene, 320, 320)
            .expect("320x320 CPU reference capture must render");
        assert_eq!(pixels.len(), 320 * 320 * 4);
        u64::from_le_bytes(
            receipt.pixel_digest[..8]
                .try_into()
                .expect("digest prefix has 8 bytes"),
        )
    }));

    measurements.push(measure("sealed_evidence_capture_128", options, || {
        let artifact =
            capture_ouroboros_evidence(&trace, plan, 128, 128).expect("evidence capture must seal");
        u64::from_le_bytes(
            artifact.receipt.bundle_digest[..8]
                .try_into()
                .expect("digest prefix has 8 bytes"),
        )
    }));

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let source_rev = env::var("BENCH_SOURCE_REV").unwrap_or_else(|_| "not-provided".into());
    let cpu_model = env::var("BENCH_CPU_MODEL").unwrap_or_else(|_| "not-provided".into());
    let cpu_governor =
        env::var("BENCH_CPU_GOVERNOR").unwrap_or_else(|_| "not-provided".into());
    let power_profile = env::var("BENCH_POWER_PROFILE").unwrap_or_else(|_| "not-provided".into());
    let kernel = env::var("BENCH_KERNEL").unwrap_or_else(|_| "not-provided".into());
    let memory_mib = env::var("BENCH_RAM_MIB").unwrap_or_else(|_| "not-provided".into());
    let toolchain = env::var("BENCH_RUST_TOOLCHAIN").unwrap_or_else(|_| "not-provided".into());
    let gpu_model = env::var("BENCH_GPU_MODEL").unwrap_or_else(|_| "not-provided".into());
    let gpu_driver = env::var("BENCH_GPU_DRIVER").unwrap_or_else(|_| "not-provided".into());
    let compositor = env::var("BENCH_COMPOSITOR").unwrap_or_else(|_| "not-provided".into());
    let parallelism = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(0);

    println!(
        concat!(
            "{{\"type\":\"metadata\",\"schema\":\"luminous-ouroboros-cpu-benchmark-v2\",",
            "\"source_rev\":\"{}\",\"profile\":\"{}\",\"os\":\"{}\",",
            "\"arch\":\"{}\",\"cpu_model\":\"{}\",\"cpu_governor\":\"{}\",",
            "\"power_profile\":\"{}\",\"kernel\":\"{}\",\"ram_mib\":\"{}\",",
            "\"rust_toolchain\":\"{}\",\"gpu_model\":\"{}\",",
            "\"gpu_driver\":\"{}\",\"compositor\":\"{}\",\"logical_cpus\":{},"
            "\"scene_contract_version\":{},\"trace_bytes\":{},",
            "\"mesh_tier\":\"{:?}\",\"mesh_vertices\":{},\"mesh_indices\":{},",
            "\"render_dimensions\":\"320x320\",\"evidence_dimensions\":\"128x128\"}}"
        ),
        json_escape(&source_rev),
        profile,
        env::consts::OS,
        env::consts::ARCH,
        json_escape(&cpu_model),
        json_escape(&cpu_governor),
        json_escape(&power_profile),
        json_escape(&kernel),
        json_escape(&memory_mib),
        json_escape(&toolchain),
        json_escape(&gpu_model),
        json_escape(&gpu_driver),
        json_escape(&compositor),
        parallelism,
        sovereign_visual_core::contract::SCENE_CONTRACT_VERSION,
        trace_bytes.len(),
        plan.tier,
        plan.vertex_count,
        plan.index_count
    );
    for measurement in &mut measurements {
        println!(
            "{}",
            measurement.as_json(options.warmup, options.iterations)
        );
    }
}

fn main() {
    run_benchmarks(parse_options());
}
