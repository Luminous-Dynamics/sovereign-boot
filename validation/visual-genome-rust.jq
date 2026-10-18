def die($message): error("visual-genome-rust: " + $message);
def exact_keys($expected):
  (type == "object") and ((keys | sort) == ($expected | sort));
def valid_text:
  type == "string" and length > 0;
def valid_slug: type == "string" and test("^[a-z0-9]+(-[a-z0-9]+)*$");
def valid_color: type == "string" and test("^#[0-9A-Fa-f]{6}$");
def uint_between($minimum; $maximum):
  type == "number" and floor == . and . >= $minimum and . <= $maximum;
def valid_palette:
  . as $palette
  | ($palette | exact_keys(["background", "convergence", "node", "primary", "secondary"]))
    and ($palette.background | valid_color)
    and ($palette.primary | valid_color)
    and ($palette.secondary | valid_color)
    and ($palette.node | valid_color)
    and ($palette.convergence | valid_color);
def valid_growth:
  . as $growth
  | ($growth | exact_keys(["branch_length_scale", "growth_speed", "initial_branch_min", "initial_branch_variants", "max_children", "max_fork_angle_degrees", "min_children", "min_fork_angle_degrees", "pulse_gain"]))
    and ($growth.initial_branch_min | uint_between(1; 9))
    and ($growth.initial_branch_variants | uint_between(1; 9))
    and ($growth.initial_branch_min + $growth.initial_branch_variants - 1 <= 9)
    and ($growth.branch_length_scale | type == "number" and . > 0 and . <= 2)
    and ($growth.growth_speed | type == "number" and . > 0 and . <= 30)
    and ($growth.min_fork_angle_degrees | type == "number" and . >= 0 and . <= 90)
    and ($growth.max_fork_angle_degrees | type == "number" and . > $growth.min_fork_angle_degrees and . <= 90)
    and ($growth.min_children | uint_between(1; 3))
    and ($growth.max_children | uint_between(1; 3))
    and ($growth.max_children >= $growth.min_children)
    and ($growth.pulse_gain | type == "number" and . >= 0 and . <= 1);
def valid_theme_motion:
  . as $motion
  | ($motion | exact_keys(["growth", "parallax", "pulse"]))
    and ($motion.growth | type == "number" and . >= 0 and . <= 1)
    and ($motion.pulse | type == "number" and . >= 0 and . <= 1)
    and ($motion.parallax | type == "number" and . >= 0 and . <= 1);
def valid_density:
  . as $density
  | ($density | exact_keys(["max_branches", "max_particles"]))
    and ($density.max_branches | uint_between(1; 8192))
    and ($density.max_particles | uint_between(0; 768));
def valid_surfaces:
  type == "array"
    and length >= 1 and length <= 3
    and all(.[]; . == "boot" or . == "desktop" or . == "lockscreen")
    and ((unique | length) == length);
def valid_theme:
  . as $theme
  | ($theme | exact_keys(["art_direction", "density", "growth_profile", "id", "implementation_status", "live_data", "motion", "name", "palette", "reduced_motion_variant", "scene_family", "shader_variant", "surfaces"]))
    and ($theme.id | valid_slug)
    and ($theme.scene_family | valid_slug)
    and ($theme.name | valid_text)
    and ($theme.art_direction | valid_text)
    and ($theme.implementation_status | valid_text)
    and ($theme.live_data | type == "boolean")
    and ($theme.reduced_motion_variant | valid_slug)
    and ($theme.motion | valid_theme_motion)
    and ($theme.density | valid_density)
    and ($theme.surfaces | valid_surfaces)
    and ($theme.palette | valid_palette)
    and ($theme.growth_profile | valid_growth)
    and ($theme.shader_variant | uint_between(0; 9));
def valid_renderer_policy:
  . as $policy
  | ($policy | exact_keys(["boot", "desktop", "executable_theme_scripts", "full_screen_flash", "lockscreen", "network_access_from_renderer", "remote_assets", "universal_fallback"]))
    and ($policy.boot == "cpu-reference-only")
    and ($policy.desktop == "wgpu-vulkan-preferred")
    and ($policy.lockscreen == "desktop-background-adapter-only")
    and ($policy.universal_fallback == "static")
    and ($policy.remote_assets == false)
    and ($policy.executable_theme_scripts == false)
    and ($policy.network_access_from_renderer == false)
    and ($policy.full_screen_flash == false);
def valid_motion_profile:
  . as $profile
  | ($profile | exact_keys(["crossfade_ms", "frame_interval_ms", "motion_scale", "particle_budget_scale", "transition_durations_seconds"]))
    and ($profile.motion_scale | type == "number" and . >= 0 and . <= 1)
    and ($profile.particle_budget_scale | type == "number" and . >= 0 and . <= 1)
    and ($profile.frame_interval_ms | uint_between(1; 1000))
    and ($profile.transition_durations_seconds | type == "array" and length == 3 and all(.[]; type == "number" and . >= 0 and . <= 10))
    and ($profile.transition_durations_seconds[2] > 0)
    and ($profile.crossfade_ms | uint_between(50; 2000));
def valid_motion_profiles:
  . as $profiles
  | ($profiles | type) == "object"
    and (($profiles | keys | sort) == ["balanced", "cinematic", "low-power", "reduced"])
    and ($profiles.reduced.motion_scale == 0)
    and ($profiles.reduced.particle_budget_scale == 0)
    and ($profiles["low-power"].motion_scale > $profiles.reduced.motion_scale)
    and ($profiles["low-power"].motion_scale < $profiles.balanced.motion_scale)
    and ($profiles.balanced.motion_scale < $profiles.cinematic.motion_scale)
    and ($profiles.cinematic.motion_scale == 1)
    and ($profiles["low-power"].particle_budget_scale > $profiles.reduced.particle_budget_scale)
    and ($profiles["low-power"].particle_budget_scale < $profiles.balanced.particle_budget_scale)
    and ($profiles.balanced.particle_budget_scale < $profiles.cinematic.particle_budget_scale)
    and ($profiles.cinematic.particle_budget_scale == 1)
    and (all($profiles[]; valid_motion_profile));
def pascal:
  reduce (split("-")[]) as $part (""; . + ($part[0:1] | ascii_upcase) + $part[1:]);
def f32lit($value):
  ($value | tostring) as $literal
  | if ($literal | test("[.eE]")) then $literal else $literal + ".0" end;
def rgb($color):
  "Rgba(0x" + ($color[1:3] | ascii_downcase)
  + ", 0x" + ($color[3:5] | ascii_downcase)
  + ", 0x" + ($color[5:7] | ascii_downcase)
  + ", 0xff)";
def contract($theme):
  "    ThemeContract {\n"
  + "        theme: Theme::" + ($theme.id | pascal) + ",\n"
  + "        id: " + ($theme.id | tojson) + ",\n"
  + "        scene_kind: SceneKind::" + ($theme.scene_family | pascal) + ",\n"
  + "        scene_family_id: " + ($theme.scene_family | tojson) + ",\n"
  + "        boot_allowed: " + ($theme.surfaces | if index("boot") == null then "false" else "true" end) + ",\n"
  + "        palette: Palette {\n"
  + "            background: " + rgb($theme.palette.background) + ",\n"
  + "            primary: " + rgb($theme.palette.primary) + ",\n"
  + "            secondary: " + rgb($theme.palette.secondary) + ",\n"
  + "            node: " + rgb($theme.palette.node) + ",\n"
  + "            convergence: " + rgb($theme.palette.convergence) + ",\n"
  + "        },\n"
  + "        growth_profile: GrowthProfile {\n"
  + "            initial_branch_min: " + ($theme.growth_profile.initial_branch_min | tostring) + ",\n"
  + "            initial_branch_variants: " + ($theme.growth_profile.initial_branch_variants | tostring) + ",\n"
  + "            branch_length_scale: " + f32lit($theme.growth_profile.branch_length_scale) + ",\n"
  + "            growth_speed: " + f32lit($theme.growth_profile.growth_speed) + ",\n"
  + "            min_fork_angle_degrees: " + f32lit($theme.growth_profile.min_fork_angle_degrees) + ",\n"
  + "            max_fork_angle_degrees: " + f32lit($theme.growth_profile.max_fork_angle_degrees) + ",\n"
  + "            min_children: " + ($theme.growth_profile.min_children | tostring) + ",\n"
  + "            max_children: " + ($theme.growth_profile.max_children | tostring) + ",\n"
  + "            pulse_gain: " + f32lit($theme.growth_profile.pulse_gain) + ",\n"
  + "        },\n"
  + "        density: ThemeDensity {\n"
  + "            max_branches: " + ($theme.density.max_branches | tostring) + ",\n"
  + "            max_particles: " + ($theme.density.max_particles | tostring) + ",\n"
  + "        },\n"
  + "        motion: ThemeMotion {\n"
  + "            growth: " + f32lit($theme.motion.growth) + ",\n"
  + "            pulse: " + f32lit($theme.motion.pulse) + ",\n"
  + "            parallax: " + f32lit($theme.motion.parallax) + ",\n"
  + "        },\n"
  + "    }";
def motion_contract($entry):
  "    MotionModeContract {\n"
  + "        mode: MotionMode::" + ($entry.key | pascal) + ",\n"
  + "        id: " + ($entry.key | tojson) + ",\n"
  + "        motion_scale: " + f32lit($entry.value.motion_scale) + ",\n"
  + "        particle_budget_scale: " + f32lit($entry.value.particle_budget_scale) + ",\n"
  + "        frame_interval_ms: " + ($entry.value.frame_interval_ms | tostring) + ",\n"
  + "        transition_durations_seconds: ("
  + f32lit($entry.value.transition_durations_seconds[0]) + ", "
  + f32lit($entry.value.transition_durations_seconds[1]) + ", "
  + f32lit($entry.value.transition_durations_seconds[2]) + "),\n"
  + "        crossfade_ms: " + ($entry.value.crossfade_ms | tostring) + ",\n"
  + "    }";
if ((. | exact_keys(["description", "motion_profiles", "renderer_policy", "schema_version", "status", "themes"])) | not) then die("top-level keys do not match schema v1")
elif (.schema_version != 1) then die("schema_version must equal 1")
elif ((.description | valid_text) | not) then die("description must be non-empty text")
elif ((.status | valid_text) | not) then die("status must be non-empty text")
elif ((.themes | type) != "array") then die("themes must be an array")
elif ((.themes | length) != 10) then die("schema v1 requires exactly ten theme contracts")
elif (([.themes[].id] | unique | length) != 10) then die("theme IDs must be unique")
elif (([.themes[].scene_family] | unique | length) != 10) then die("scene-family IDs must be unique")
elif (([.themes[].shader_variant] | unique | length) != 10) then die("shader variants must be unique")
elif ((.renderer_policy | valid_renderer_policy) | not) then die("renderer policy is invalid or contains undeclared fields")
elif ((.motion_profiles | valid_motion_profiles) | not) then die("motion profiles are incomplete or out of range")
elif (all(.themes[]; valid_theme) | not) then die("theme contract has invalid, missing or undeclared fields")
else
  . as $manifest
  | $manifest.themes as $themes
  | $manifest.motion_profiles as $motions
  | "// @generated from themes/visual-genome-v1.json by validation/visual-genome-rust.jq.\n"
    + "// Do not edit by hand; regenerate and commit this file with manifest changes.\n\n"
    + "const GENERATED_THEME_ALL: [Theme; 10] = [\n"
    + ($themes | map("    Theme::" + (.id | pascal) + ",") | join("\n"))
    + "\n];\n\n"
    + "const GENERATED_THEME_CONTRACTS: [ThemeContract; 10] = [\n"
    + ($themes | map(. as $theme | contract($theme)) | join(",\n"))
    + "\n];\n\n"
    + "const GENERATED_MOTION_MODE_ALL: [MotionMode; 4] = [\n"
    + ($motions | to_entries | sort_by(.value.motion_scale) | map("    MotionMode::" + (.key | pascal) + ",") | join("\n"))
    + "\n];\n\n"
    + "const GENERATED_MOTION_MODE_CONTRACTS: [MotionModeContract; 4] = [\n"
    + ($motions | to_entries | sort_by(.value.motion_scale) | map(. as $entry | motion_contract($entry)) | join(",\n"))
    + "\n];"
end

