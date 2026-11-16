use std::error::Error;
use std::fmt;

#[derive(Clone, Debug)]
struct Config {
    scales: u8,
    finest_nodes: u32,
    max_nodes: usize,
    max_steps: u64,
    dt: f64,
    coupling: f64,
    decay: f64,
    drive: f64,
    initial_resource: f64,
    idle_cost: f64,
    activity_cost: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scales: 3,
            finest_nodes: 16,
            max_nodes: 128,
            max_steps: 10_000,
            dt: 0.02,
            coupling: 0.18,
            decay: 0.40,
            drive: 0.03,
            initial_resource: 1.0,
            idle_cost: 0.001,
            activity_cost: 0.002,
        }
    }
}

impl Config {
    fn validate(&self) -> Result<(), LabError> {
        if !(1..=6).contains(&self.scales) {
            return Err(LabError::InvalidConfig("scales must be in 1..=6"));
        }
        if !(2..=64).contains(&self.finest_nodes) {
            return Err(LabError::InvalidConfig("finest_nodes must be in 2..=64"));
        }
        if self.max_steps == 0 {
            return Err(LabError::InvalidConfig("max_steps must be nonzero"));
        }
        if !self.dt.is_finite() || self.dt <= 0.0 || self.dt > 0.05 {
            return Err(LabError::InvalidConfig("dt must be finite and in (0, 0.05]"));
        }
        for (name, value, min, max) in [
            ("coupling", self.coupling, 0.0, 1.0),
            ("decay", self.decay, 0.0, 2.0),
            ("drive", self.drive, 0.0, 1.0),
        ] {
            if !value.is_finite() || value < min || value > max {
                return Err(LabError::InvalidConfig(match name {
                    "coupling" => "coupling must be finite and in [0, 1]",
                    "decay" => "decay must be finite and in [0, 2]",
                    _ => "drive must be finite and in [0, 1]",
                }));
            }
        }
        if !self.initial_resource.is_finite() || self.initial_resource <= 0.0 {
            return Err(LabError::InvalidConfig("initial_resource must be finite and positive"));
        }
        for (name, value) in [
            ("idle_cost", self.idle_cost),
            ("activity_cost", self.activity_cost),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(LabError::InvalidConfig(match name {
                    "idle_cost" => "idle_cost must be finite and nonnegative",
                    _ => "activity_cost must be finite and nonnegative",
                }));
            }
        }
        if self.node_count()? > self.max_nodes {
            return Err(LabError::NodeBudgetExceeded {
                required: self.node_count()?,
                maximum: self.max_nodes,
            });
        }
        Ok(())
    }

    fn node_count(&self) -> Result<usize, LabError> {
        let mut total = 0usize;
        for level in 0..self.scales {
            let count = (self.finest_nodes >> level).max(2) as usize;
            total = total.checked_add(count).ok_or(LabError::ArithmeticOverflow)?;
        }
        Ok(total)
    }
}

#[derive(Clone, Debug)]
struct Node {
    id: u32,
    scale: u8,
    activity: f64,
    resource: f64,
}

#[derive(Clone, Debug)]
struct Lattice {
    config: Config,
    nodes: Vec<Node>,
    neighbors: Vec<Vec<usize>>,
    seed: u64,
    tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LabError {
    InvalidConfig(&'static str),
    NodeBudgetExceeded { required: usize, maximum: usize },
    StepBudgetExceeded { requested: u64, maximum: u64 },
    ResourceExhausted { node_id: u32 },
    UnknownNode(u32),
    ArithmeticOverflow,
}

impl fmt::Display for LabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(f, "invalid config: {message}"),
            Self::NodeBudgetExceeded { required, maximum } => {
                write!(f, "node budget exceeded: required {required}, maximum {maximum}")
            }
            Self::StepBudgetExceeded { requested, maximum } => {
                write!(f, "step budget exceeded: requested through {requested}, maximum {maximum}")
            }
            Self::ResourceExhausted { node_id } => {
                write!(f, "resource exhausted at node {node_id}; step was not committed")
            }
            Self::UnknownNode(id) => write!(f, "unknown node {id}"),
            Self::ArithmeticOverflow => write!(f, "integer arithmetic overflow"),
        }
    }
}

impl Error for LabError {}

impl Lattice {
    fn new(config: Config, seed: u64) -> Result<Self, LabError> {
        config.validate()?;

        let mut nodes = Vec::with_capacity(config.node_count()?);
        let mut level_ranges: Vec<(usize, usize)> = Vec::with_capacity(config.scales as usize);

        for scale in 0..config.scales {
            let count = (config.finest_nodes >> scale).max(2) as usize;
            let start = nodes.len();

            for _ in 0..count {
                let id = u32::try_from(nodes.len()).map_err(|_| LabError::ArithmeticOverflow)?;
                let mixed = mix64(seed ^ u64::from(id));
                let fraction = (mixed % 500_001) as f64 / 1_000_000.0;
                nodes.push(Node {
                    id,
                    scale,
                    activity: 0.25 + fraction,
                    resource: config.initial_resource,
                });
            }

            level_ranges.push((start, nodes.len()));
        }

        let mut neighbors = vec![Vec::<usize>::new(); nodes.len()];

        // Local ring edges at each scale.
        for &(start, end) in &level_ranges {
            let count = end - start;
            for local in 0..count {
                let a = start + local;
                let b = start + ((local + 1) % count);
                add_undirected_edge(&mut neighbors, a, b);
            }
        }

        // Each finer-scale node links to one parent on the next coarser scale.
        for level in 0..level_ranges.len().saturating_sub(1) {
            let (fine_start, fine_end) = level_ranges[level];
            let (coarse_start, coarse_end) = level_ranges[level + 1];
            let coarse_count = coarse_end - coarse_start;

            for fine_local in 0..(fine_end - fine_start) {
                let parent_local = (fine_local / 2).min(coarse_count - 1);
                add_undirected_edge(
                    &mut neighbors,
                    fine_start + fine_local,
                    coarse_start + parent_local,
                );
            }
        }

        for adjacent in &mut neighbors {
            adjacent.sort_unstable();
            adjacent.dedup();
        }

        Ok(Self {
            config,
            nodes,
            neighbors,
            seed,
            tick: 0,
        })
    }

    fn step(&mut self) -> Result<(), LabError> {
        let next_tick = self.tick.checked_add(1).ok_or(LabError::ArithmeticOverflow)?;
        if next_tick > self.config.max_steps {
            return Err(LabError::StepBudgetExceeded {
                requested: next_tick,
                maximum: self.config.max_steps,
            });
        }

        // Compute the entire candidate transition before mutating state.
        // This makes resource exhaustion an atomic rejected step.
        let mut next_activity = Vec::with_capacity(self.nodes.len());
        let mut next_resource = Vec::with_capacity(self.nodes.len());

        for (index, node) in self.nodes.iter().enumerate() {
            let adjacent = &self.neighbors[index];
            let mean_neighbor = if adjacent.is_empty() {
                0.0
            } else {
                adjacent.iter().map(|&j| self.nodes[j].activity).sum::<f64>()
                    / adjacent.len() as f64
            };

            let scale_drive = self.config.drive * (1.0 + 0.05 * f64::from(node.scale));
            let derivative = scale_drive
                + self.config.coupling * mean_neighbor
                - self.config.decay * node.activity;
            let activity = (node.activity + self.config.dt * derivative).clamp(0.0, 1.0);

            let cost = self.config.dt
                * (self.config.idle_cost + self.config.activity_cost * node.activity);
            let resource = node.resource - cost;

            if !activity.is_finite() || !resource.is_finite() {
                return Err(LabError::InvalidConfig("transition produced non-finite state"));
            }
            if resource < 0.0 {
                return Err(LabError::ResourceExhausted { node_id: node.id });
            }

            next_activity.push(activity);
            next_resource.push(resource);
        }

        for (index, node) in self.nodes.iter_mut().enumerate() {
            node.activity = next_activity[index];
            node.resource = next_resource[index];
        }
        self.tick = next_tick;
        Ok(())
    }

    fn run_steps(&mut self, steps: u64) -> Result<(), LabError> {
        let final_tick = self.tick.checked_add(steps).ok_or(LabError::ArithmeticOverflow)?;
        if final_tick > self.config.max_steps {
            return Err(LabError::StepBudgetExceeded {
                requested: final_tick,
                maximum: self.config.max_steps,
            });
        }
        for _ in 0..steps {
            self.step()?;
        }
        Ok(())
    }

    fn perturb_node_activity(&mut self, node_id: u32, activity: f64) -> Result<(), LabError> {
        if !activity.is_finite() || !(0.0..=1.0).contains(&activity) {
            return Err(LabError::InvalidConfig("perturbed activity must be finite and in [0, 1]"));
        }
        let node = self.nodes.iter_mut().find(|node| node.id == node_id)
            .ok_or(LabError::UnknownNode(node_id))?;
        node.activity = activity;
        Ok(())
    }

    fn activity(&self, node_id: u32) -> Option<f64> {
        self.nodes.iter().find(|node| node.id == node_id).map(|node| node.activity)
    }

    fn mean_activity(&self) -> f64 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        self.nodes.iter().map(|node| node.activity).sum::<f64>() / self.nodes.len() as f64
    }

    fn min_resource(&self) -> f64 {
        self.nodes.iter().map(|node| node.resource).fold(f64::INFINITY, f64::min)
    }

    // FNV-1a is used only as a compact same-build replay comparison.
    // This digest is NOT cryptographic and must not authenticate state.
    fn replay_digest_non_security(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;

        // Bind the comparison to the complete experiment identity, not only
        // the visible node values. This remains a non-cryptographic digest.
        fnv_mix(&mut hash, &self.seed.to_le_bytes());
        fnv_mix(&mut hash, &[self.config.scales]);
        fnv_mix(&mut hash, &self.config.finest_nodes.to_le_bytes());
        fnv_mix(&mut hash, &(self.config.max_nodes as u64).to_le_bytes());
        fnv_mix(&mut hash, &self.config.max_steps.to_le_bytes());
        for value in [
            self.config.dt,
            self.config.coupling,
            self.config.decay,
            self.config.drive,
            self.config.initial_resource,
            self.config.idle_cost,
            self.config.activity_cost,
        ] {
            fnv_mix(&mut hash, &value.to_bits().to_le_bytes());
        }
        fnv_mix(&mut hash, &self.tick.to_le_bytes());

        for (index, adjacent) in self.neighbors.iter().enumerate() {
            fnv_mix(&mut hash, &(index as u64).to_le_bytes());
            fnv_mix(&mut hash, &(adjacent.len() as u64).to_le_bytes());
            for &neighbor in adjacent {
                fnv_mix(&mut hash, &(neighbor as u64).to_le_bytes());
            }
        }
        for node in &self.nodes {
            fnv_mix(&mut hash, &node.id.to_le_bytes());
            fnv_mix(&mut hash, &[node.scale]);
            fnv_mix(&mut hash, &node.activity.to_bits().to_le_bytes());
            fnv_mix(&mut hash, &node.resource.to_bits().to_le_bytes());
        }
        hash
    }
}

fn add_undirected_edge(neighbors: &mut [Vec<usize>], a: usize, b: usize) {
    if a != b {
        neighbors[a].push(b);
        neighbors[b].push(a);
    }
}

fn fnv_mix(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn mix64(mut x: u64) -> u64 {
    // SplitMix64 finalizer: deterministic initializer only, not a CSPRNG.
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn run_demo() -> Result<(), LabError> {
    let config = Config::default();
    let mut baseline = Lattice::new(config.clone(), 0x4848_4c46)?;
    baseline.run_steps(64)?;

    let mut perturbed = Lattice::new(config, 0x4848_4c46)?;
    perturbed.perturb_node_activity(0, 0.0)?;
    perturbed.run_steps(64)?;

    println!("HHLF-inspired Living Lattice — research scaffold");
    println!("nodes={}", perturbed.nodes.len());
    println!("ticks={}", perturbed.tick);
    println!("baseline_mean_activity={:.6}", baseline.mean_activity());
    println!("perturbed_mean_activity={:.6}", perturbed.mean_activity());
    println!("perturbed_node_0_activity={:.6}", perturbed.activity(0).unwrap_or(0.0));
    println!("minimum_resource={:.6}", perturbed.min_resource());
    println!("baseline_replay_digest_non_security={:016x}", baseline.replay_digest_non_security());
    println!("perturbed_replay_digest_non_security={:016x}", perturbed.replay_digest_non_security());
    println!("claim_ceiling=deterministic bounded scalar lattice only; not HDC, fractal proof, life, crypto, or boot security");
    Ok(())
}

fn main() {
    if let Err(error) = run_demo() {
        eprintln!("research run failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_and_config_replay_identically() {
        let config = Config::default();
        let mut left = Lattice::new(config.clone(), 1234).unwrap();
        let mut right = Lattice::new(config, 1234).unwrap();
        left.run_steps(100).unwrap();
        right.run_steps(100).unwrap();
        assert_eq!(left.replay_digest_non_security(), right.replay_digest_non_security());
    }

    #[test]
    fn replay_digest_binds_seed_and_configuration() {
        let config = Config::default();
        let same_state_a = Lattice::new(config.clone(), 1234).unwrap();
        let different_seed = Lattice::new(config.clone(), 5678).unwrap();
        let mut changed_config = config;
        changed_config.drive = 0.04;
        let different_config = Lattice::new(changed_config, 1234).unwrap();

        assert_ne!(
            same_state_a.replay_digest_non_security(),
            different_seed.replay_digest_non_security()
        );
        assert_ne!(
            same_state_a.replay_digest_non_security(),
            different_config.replay_digest_non_security()
        );
    }

    #[test]
    fn perturbed_activity_recovers_from_zero() {
        let mut lattice = Lattice::new(Config::default(), 99).unwrap();
        lattice.perturb_node_activity(0, 0.0).unwrap();
        lattice.run_steps(64).unwrap();
        assert!(lattice.activity(0).unwrap() > 0.0);
    }

    #[test]
    fn rejected_resource_step_does_not_partially_mutate_state() {
        let mut config = Config::default();
        config.initial_resource = 1.0e-9;
        let mut lattice = Lattice::new(config, 7).unwrap();
        let before = lattice.replay_digest_non_security();
        let result = lattice.step();
        assert!(matches!(result, Err(LabError::ResourceExhausted { .. })));
        assert_eq!(before, lattice.replay_digest_non_security());
    }

    #[test]
    fn step_budget_is_explicit() {
        let mut config = Config::default();
        config.max_steps = 2;
        let mut lattice = Lattice::new(config, 7).unwrap();
        assert!(matches!(
            lattice.run_steps(3),
            Err(LabError::StepBudgetExceeded { .. })
        ));
        assert_eq!(lattice.tick, 0);
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        let mut config = Config::default();
        config.dt = f64::NAN;
        assert!(matches!(
            Lattice::new(config, 1),
            Err(LabError::InvalidConfig(_))
        ));
    }

    #[test]
    fn node_budget_is_enforced() {
        let mut config = Config::default();
        config.max_nodes = 1;
        assert!(matches!(
            Lattice::new(config, 1),
            Err(LabError::NodeBudgetExceeded { .. })
        ));
    }
}
