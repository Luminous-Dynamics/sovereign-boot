def die($message): error("visual-genome-rust: " + $message);
def valid_slug: type == "string" and test("^[a-z0-9]+(-[a-z0-9]+)*$");
def valid_color: type == "string" and test("^#[0-9A-Fa-f]{6}$");
def uint_between($minimum; $maximum):
  type == "number" and floor == . and . >= $minimum and . <= $maximum;
def valid_palette:
  . as $palette
  | ($palette | type) == "object"
    and ($palette.background | valid_color)
    and ($palette.primary | valid_color)
    and ($palette.secondary | valid_color)
    and ($palette.node | valid_color)
    and ($palette.convergence | valid_color);
def valid_growth:
  . as $growth
  | ($growth | type) == "object"
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
def valid_motion_profile:
  . as $profile
  | ($profile | type) == "object"
    and ($profile.motion_scale | type == "number" and . >= 0 and . <= 1)
    and ($profile.particle_budget_scale | type == "number" and . >= 0 and . <= 1)
    and ($profile.frame_interval_ms | uint_between(1; 1000))
    and ($profile.transition_durations_seconds | type == "array" and length == 3 and all(.[]; type == "number" and . >= 0 and . <= 10))
    and ($profile.transition_durations_seconds[2] > 0)
    and ($profile.crossfade_ms | uint_between(1; 2000));
def valid_motion_profiles:
  . as $profiles
  | ($profiles | type) == "object"
    and (($profiles | keys | sort) == ["balanced", "cinematic", "low-power", "reduced"])
    and ($profiles.reduced.motion_scale == 0)
    and ($profiles.reduced.particle_budget_scale == 0)
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
if (.schema_version != 1) then die("schema_version must equal 1")
elif ((.themes | type) != "array") then die("themes must be an array")
elif ((.themes | length) != 10) then die("schema v1 requires exactly ten theme contracts")
elif (([.themes[].id] | unique | length) != 10) then die("theme IDs must be unique")
elif (([.themes[].scene_family] | unique | length) != 10) then die("scene-family IDs must be unique")
elif ((.motion_profiles | valid_motion_profiles) | not) then die("motion profiles are incomplete or out of range")
elif (all(.themes[];
  (.id | valid_slug)
  and (.scene_family | valid_slug)
  and (.palette | valid_palette)
  and (.growth_profile | valid_growth)
  and (.shader_variant | uint_between(0; 255))
) | not) then die("theme contract has invalid slug, palette, growth profile or shader variant")
else
  . as $manifest
  | $manifest.themes as $themes
  | $manifest.motion_profiles as $motions
  | "// @generated from themes/visual-genome-v1.json by validation/visual-genome-rust.jq.\n"
    + "// Do not edit by hand; regenerate and commit this file with manifest changes.\n\n"
    + "pub(super) const GENERATED_THEME_ALL: [Theme; 10] = [\n"
    + ($themes | map("    Theme::" + (.id | pascal) + ",") | join("\n"))
    + "\n];\n\n"
    + "pub(super) const GENERATED_THEME_CONTRACTS: [ThemeContract; 10] = [\n"
    + ($themes | map(. as $theme | contract($theme)) | join(",\n"))
    + "\n];\n\n"
    + "pub(super) const GENERATED_MOTION_MODE_ALL: [MotionMode; 4] = [\n"
    + ($motions | to_entries | sort_by(.value.motion_scale) | map("    MotionMode::" + (.key | pascal) + ",") | join("\n"))
    + "\n];\n\n"
    + "pub(super) const GENERATED_MOTION_MODE_CONTRACTS: [MotionModeContract; 4] = [\n"
    + ($motions | to_entries | sort_by(.value.motion_scale) | map(. as $entry | motion_contract($entry)) | join(",\n"))
    + "\n];\n"
end

