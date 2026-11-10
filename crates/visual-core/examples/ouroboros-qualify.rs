//! Reproducible host-side qualification harness for Ouroboros core hot paths.
//!
//! Run with:
//!   cargo run -p sovereign-visual-core --example ouroboros-qualify --release -- 200
//!
//! Output is JSON containing raw p50/p95/p99 latency samples. This measures
//! the Rust reference implementation on the executing host; it is not a GPU,
//! compositor, power or cross-product benchmark.

use std::{env, hint::black_box, time::Instant};

use sovereign_visual_core::{
    ouroboros::{
        AttentionTarget, OuroborosConfig, OuroborosForm, OuroborosMotionMode, OuroborosScene,
    },
    ouroboros_evidence::capture_ouroboros_evidence,
    ouroboros_face::{OuroborosFaceMesh, OuroborosFaceMeshBudget, OuroborosFaceMeshConfig},
    ouroboros_mesh::{OuroborosMesh, OuroborosMeshBudget, OuroborosMeshConfig},
    ouroboros_render::render_reference_artifact,
    ouroboros_trace::{OuroborosEvent, OuroborosTrace},
};

#[derive(Debug)]
struct BenchStats {
    samples: usize,
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
    min_ns: u128,
    max_ns: u128,
    mean_ns: u128,
}

fn percentile(sorted: &[u128], percent: usize) -> u128 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn benchmark<F, T>(iterations: usize, mut operation: F) -> BenchStats
where
    F: FnMut() -> T,
{
    let mut durations = Vec::with_capacity(iterations);
    let mut total = 0_u128;

    for _ in 0..iterations {
        let start = Instant::now();
        black_box(operation());
        let elapsed = start.elapsed().as_nanos();
        total = total.saturating_add(elapsed);
        durations.push(elapsed);
    }

    durations.sort_unstable();
    BenchStats {
        samples: durations.len(),
        p50_ns: percentile(&durations, 50),
        p95_ns: percentile(&durations, 95),
        p99_ns: percentile(&durations, 99),
        min_ns: durations[0],
        max_ns: durations[durations.len() - 1],
        mean_ns: total / durations.len() as u128,
    }
}

fn print_stat(name: &str, stats: &BenchStats) {
    println!(
        "    \"{name}\": {{\"samples\": {}, \"p50_ns\": {}, \"p95_ns\": {}, \"p99_ns\": {}, \"min_ns\": {}, \"max_ns\": {}, \"mean_ns\": {}}},",
        stats.samples,
        stats.p50_ns,
        stats.p95_ns,
        stats.p99_ns,
        stats.min_ns,
        stats.max_ns,
        stats.mean_ns
    );
}

fn build_trace() -> OuroborosTrace {
    let mut trace = OuroborosTrace::new(OuroborosConfig::default(), OuroborosForm::Nature);
    trace
        .push(OuroborosEvent::TransitionTo {
            form: OuroborosForm::Digital,
        })
        .expect("transition event is valid");
    for _ in 0..120 {
        trace
            .push(OuroborosEvent::Advance {
                dt_seconds: 1.0 / 60.0,
            })
            .expect("fixed step is valid");
    }
    trace
        .push(OuroborosEvent::SetAttentionTarget {
            target: AttentionTarget::new(0.35, -0.25).expect("normalized attention is valid"),
        })
        .expect("attention event is valid");
    trace
        .push(OuroborosEvent::SetMotionMode {
            mode: OuroborosMotionMode::Balanced,
        })
        .expect("motion mode event is valid");
    trace
}

fn main() {
    let iterations = env::args()
        .nth(1)
        .map(|value| {
            value
                .parse::<usize>()
                .expect("iterations must be an integer")
        })
        .unwrap_or(200);
    assert!(
        (10..=50_000).contains(&iterations),
        "iterations must be in 10..=50000"
    );

    let config = OuroborosConfig::default();
    let mut step_scene =
        OuroborosScene::new(config, OuroborosForm::Nature).expect("valid default scene");
    let mut step_index = 0_u32;
    let step_stats = benchmark(iterations, || {
        if step_index.is_multiple_of(120) {
            let target = if (step_index / 120).is_multiple_of(2) {
                OuroborosForm::Digital
            } else {
                OuroborosForm::Nature
            };
            step_scene.transition_to(target);
        }
        step_scene
            .advance(1.0 / 60.0)
            .expect("fixed 60 Hz step is valid");
        step_index = step_index.wrapping_add(1);
        step_scene.frame()
    });

    let mut landmark_scene =
        OuroborosScene::new(config, OuroborosForm::Humanity).expect("valid default scene");
    landmark_scene
        .set_attention_target(AttentionTarget::new(0.8, -0.6).unwrap())
        .expect("attention target is valid");
    for _ in 0..60 {
        landmark_scene.advance(1.0 / 60.0).unwrap();
    }
    let landmark_stats = benchmark(iterations, || landmark_scene.landmarks().receipt());

    let mut checkpoint_scene =
        OuroborosScene::new(config, OuroborosForm::Nature).expect("valid default scene");
    checkpoint_scene.transition_to(OuroborosForm::Enlightenment);
    for _ in 0..60 {
        checkpoint_scene.advance(1.0 / 60.0).unwrap();
    }
    let checkpoint_stats = benchmark(iterations, || {
        let (bytes, receipt) = checkpoint_scene.encode_checkpoint().unwrap();
        let (restored, restored_receipt) = OuroborosScene::restore_checkpoint(&bytes).unwrap();
        black_box(restored.frame());
        assert_eq!(receipt, restored_receipt);
        receipt.digest
    });

    let trace = build_trace();
    let replay_scene = trace.replay().expect("benchmark trace replays").scene;
    let budget = OuroborosMeshBudget {
        projected_major_radius_px: 60,
        projected_tube_radius_px: 4,
        max_vertices: 10_000,
        max_indices: 60_000,
        max_buffer_bytes: 1_000_000,
    };
    let plan = OuroborosMeshConfig::plan_for_budget(&replay_scene, 0.025, budget)
        .expect("benchmark mesh budget is feasible");
    let lod_stats = benchmark(iterations, || {
        OuroborosMeshConfig::plan_for_budget(&replay_scene, 0.025, budget)
            .expect("fixed benchmark LOD inputs remain valid")
    });
    let face_budget = OuroborosFaceMeshBudget {
        projected_face_radius_px: 32,
        max_vertices: 50_000,
        max_indices: 300_000,
        max_buffer_bytes: 2_500_000,
    };
    let face_plan = OuroborosFaceMeshConfig::plan_for_budget(face_budget)
        .expect("benchmark face LOD budget is feasible");
    let face_lod_stats = benchmark(iterations, || {
        OuroborosFaceMeshConfig::plan_for_budget(face_budget)
            .expect("fixed benchmark face LOD inputs remain valid")
    });
    let face_mesh_stats = benchmark(iterations, || {
        let (mesh, receipt) =
            OuroborosFaceMesh::build_with_plan(&replay_scene, replay_scene.landmarks(), face_plan)
                .expect("budgeted face mesh builds");
        mesh.validate(&replay_scene).expect("face mesh validates");
        assert_eq!(mesh.receipt().unwrap(), receipt);
        receipt.digest
    });
    let mesh_stats = benchmark(iterations, || {
        let (mesh, receipt) =
            OuroborosMesh::build(&replay_scene, plan.config).expect("benchmark mesh builds");
        mesh.validate().expect("benchmark mesh validates");
        assert_eq!(mesh.receipt().unwrap(), receipt);
        receipt.digest
    });

    let mut render_scene =
        OuroborosScene::new(config, OuroborosForm::Enlightenment).expect("valid default scene");
    render_scene
        .set_attention_target(AttentionTarget::new(-0.4, 0.3).unwrap())
        .unwrap();
    for _ in 0..60 {
        render_scene.advance(1.0 / 60.0).unwrap();
    }
    let render_stats = benchmark(iterations, || {
        let (pixels, receipt) = render_reference_artifact(&render_scene, 256, 256)
            .expect("CPU reference capture succeeds");
        assert_eq!(pixels.len(), 256 * 256 * 4);
        receipt.pixel_digest
    });

    let evidence_iterations = iterations.min(100);
    let evidence_stats = benchmark(evidence_iterations, || {
        let artifact = capture_ouroboros_evidence(&trace, plan, face_plan, 128, 128)
            .expect("sealed evidence capture succeeds");
        black_box(artifact.receipt.bundle_digest)
    });

    let empty_stats = benchmark(iterations, || black_box(0_u64));

    println!("{{");
    println!("  \"schema\": \"ouroboros-host-qualification-v1\",");
    println!(
        "  \"build\": {{\"package\": \"sovereign-visual-core\", \"version\": \"{}\", \"profile\": \"{}\", \"target_arch\": \"{}\", \"target_os\": \"{}\", \"git_commit\": \"{}\"}},",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        env::consts::ARCH,
        env::consts::OS,
        option_env!("GIT_COMMIT").unwrap_or("unspecified")
    );
    println!("  \"inputs\": {{");
    println!("    \"requested_iterations\": {iterations},");
    println!("    \"evidence_iterations\": {evidence_iterations},");
    println!("    \"cpu_capture_extent\": [256, 256],");
    println!("    \"evidence_capture_extent\": [128, 128],");
    println!(
        "    \"mesh_plan\": {{\"tier\": \"{:?}\", \"ring_segments\": {}, \"tube_segments\": {}, \"vertices\": {}, \"indices\": {}, \"estimated_buffer_bytes\": {}}}",
        plan.tier,
        plan.config.ring_segments,
        plan.config.tube_segments,
        plan.vertex_count,
        plan.index_count,
        plan.estimated_buffer_bytes
    );
    println!("  }},");
    println!("  \"latency\": {{");
    print_stat("empty_control", &empty_stats);
    print_stat("scene_step_60hz", &step_stats);
    print_stat("landmark_receipt", &landmark_stats);
    print_stat("checkpoint_encode_restore", &checkpoint_stats);
    print_stat("host_budget_lod_plan", &lod_stats);
    print_stat("face_lod_plan", &face_lod_stats);
    print_stat("mesh_build_validate_receipt", &mesh_stats);
    print_stat("face_mesh_build_validate_receipt", &face_mesh_stats);
    print_stat("cpu_rgba_256", &render_stats);
    print_stat("sealed_evidence_128", &evidence_stats);
    println!("  }}");
    println!("}}");
}
