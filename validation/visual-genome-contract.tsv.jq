.themes[]
| [
    .id,
    .scene_family,
    (.palette.background | ascii_downcase),
    (.palette.primary | ascii_downcase),
    (.palette.secondary | ascii_downcase),
    (.palette.node | ascii_downcase),
    (.palette.convergence | ascii_downcase),
    .growth_profile.initial_branch_min,
    .growth_profile.initial_branch_variants,
    .growth_profile.branch_length_scale,
    .growth_profile.growth_speed,
    .growth_profile.min_fork_angle_degrees,
    .growth_profile.max_fork_angle_degrees,
    .growth_profile.min_children,
    .growth_profile.max_children,
    .growth_profile.pulse_gain,
    .density.max_branches,
    .density.max_particles,
    .motion.growth,
    .motion.pulse,
    .motion.parallax,
    (.surfaces | index("boot") != null),
    (.surfaces | index("desktop") != null),
    (.surfaces | index("lockscreen") != null)
  ]
| @tsv
