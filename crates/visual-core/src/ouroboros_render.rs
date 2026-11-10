//! Deterministic CPU reference rasterizer for the Ouroboros scene.
//!
//! This renderer is deliberately modest and dependency-free: it creates a
//! stylized, inspectable 2D reference image from the portable scene contract.
//! It is not the future volumetric/WGPU desktop renderer. The same input must
//! produce the same RGBA bytes, making it suitable for golden fixtures,
//! reduced-motion checks and CPU/GPU semantic comparisons.

use crate::{
    color::Rgba,
    contract::{self, DimensionsError},
    ouroboros::{OuroborosError, OuroborosScene},
};

const BODY_SAMPLES: u32 = 512;

const NATURE: Rgba = Rgba(112, 205, 151, 255);
const HUMANITY: Rgba = Rgba(232, 179, 151, 255);
const DIGITAL: Rgba = Rgba(104, 177, 246, 255);
const ENLIGHTENMENT: Rgba = Rgba(247, 226, 177, 255);
const DARK_INK: Rgba = Rgba(15, 25, 30, 255);
const BACKGROUND: Rgba = Rgba(8, 17, 24, 255);
const STARLIGHT: Rgba = Rgba(220, 232, 223, 255);

/// Failure while creating a bounded reference image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosRenderError {
    InvalidDimensions(DimensionsError),
    Scene(OuroborosError),
    BufferSizeOverflow,
}

impl std::fmt::Display for OuroborosRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDimensions(error) => write!(f, "invalid output dimensions: {error:?}"),
            Self::Scene(error) => write!(f, "invalid Ouroboros scene: {error}"),
            Self::BufferSizeOverflow => write!(f, "RGBA output size overflowed"),
        }
    }
}

impl std::error::Error for OuroborosRenderError {}

/// Content-addressed metadata for one CPU reference capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceImageReceipt {
    pub scene_contract_version: u16,
    pub width: u32,
    pub height: u32,
    pub pixel_digest: [u8; 32],
}

/// Render a reference capture and bind its exact dimensions and bytes to a digest.
pub fn render_reference_artifact(
    scene: &OuroborosScene,
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, ReferenceImageReceipt), OuroborosRenderError> {
    let pixels = render_reference_rgba(scene, width, height)?;
    let receipt = ReferenceImageReceipt {
        scene_contract_version: crate::contract::SCENE_CONTRACT_VERSION,
        width,
        height,
        pixel_digest: *blake3::hash(&pixels).as_bytes(),
    };
    Ok((pixels, receipt))
}

/// Render a deterministic opaque RGBA8 reference image.
///
/// The current profile draws one continuous ring, expression-specific body
/// details, and a persistent stylized face. The face pose follows only the
/// normalized attention target already supplied to the scene. No OS input,
/// camera, GPU, window-system, file or network access occurs here.
pub fn render_reference_rgba(
    scene: &OuroborosScene,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, OuroborosRenderError> {
    contract::validate_dimensions(width, height)
        .map_err(OuroborosRenderError::InvalidDimensions)?;
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(OuroborosRenderError::BufferSizeOverflow)?;
    let byte_count = pixel_count
        .checked_mul(4)
        .ok_or(OuroborosRenderError::BufferSizeOverflow)?;

    let mut pixels = vec![0_u8; byte_count];
    fill(&mut pixels, BACKGROUND);

    let frame = scene.frame();
    let landmarks = scene.landmarks();
    let weights = frame.form_weights;
    let palette = mixed_palette(weights);
    let center = [width as f32 * 0.5, height as f32 * 0.5];
    let scale = width.min(height) as f32 * 0.84;
    let config = scene.config();
    let project = |point: [f32; 3]| [center[0] + point[0] * scale, center[1] + point[1] * scale];
    let anchor = project(landmarks.head_anchor);
    let forward = normalized([landmarks.forward[0], landmarks.forward[1]]);
    let lateral = normalized([landmarks.lateral[0], landmarks.lateral[1]]);
    let head_radius = config.major_radius * scale * 0.22;

    // A subtle ambient halo is visible in every form. Enlightenment strengthens
    // it but never replaces the shared ring identity.
    let halo_strength = weights.enlightenment.clamp(0.0, 1.0);
    if halo_strength > 0.0 {
        let halo = ENLIGHTENMENT.with_opacity((24.0 + 54.0 * halo_strength) / 255.0);
        draw_circle(
            &mut pixels,
            width,
            height,
            center,
            config.major_radius * scale * 1.12,
            192,
            halo,
            0.7,
        );
    }

    // The body is a stable ordered curve across all forms. Material channels
    // change continuously while the centerline sampling identity is retained.
    let mut previous = None;
    let mut first = None;
    for index in 0..BODY_SAMPLES {
        let sample = scene
            .body_sample(index, BODY_SAMPLES)
            .map_err(OuroborosRenderError::Scene)?;
        let point = [
            center[0] + sample.centerline[0] * scale,
            center[1] + sample.centerline[1] * scale,
        ];

        if let Some(start) = previous {
            draw_line(
                &mut pixels,
                width,
                height,
                start,
                point,
                palette.with_opacity(0.18),
                7.0,
            );
            draw_line(&mut pixels, width, height, start, point, palette, 2.25);
        } else {
            first = Some(point);
        }
        previous = Some(point);
    }
    // Close the body behind the persistent head so it reads as one continuous
    // Ouroboros ring rather than an open-ended curve.
    if let (Some(last), Some(first)) = (previous, first) {
        draw_line(
            &mut pixels,
            width,
            height,
            last,
            first,
            palette.with_opacity(0.18),
            7.0,
        );
        draw_line(&mut pixels, width, height, last, first, palette, 2.25);
    }

    // Nature: paired leaf/node motifs grow from the same ring centerline.
    let nature_strength = weights.nature.clamp(0.0, 1.0);
    if nature_strength > 0.02 {
        let color = NATURE.with_opacity((80.0 + 175.0 * nature_strength) / 255.0);
        for index in (12..BODY_SAMPLES).step_by(24) {
            let sample = scene
                .body_sample(index, BODY_SAMPLES)
                .map_err(OuroborosRenderError::Scene)?;
            let point = [
                center[0] + sample.centerline[0] * scale,
                center[1] + sample.centerline[1] * scale,
            ];
            let normal = normalized([point[0] - center[0], point[1] - center[1]]);
            let tangent = [-normal[1], normal[0]];
            let leaf = [point[0] + normal[0] * 4.5, point[1] + normal[1] * 4.5];
            let stem = [leaf[0] + tangent[0] * 2.0, leaf[1] + tangent[1] * 2.0];
            draw_line(&mut pixels, width, height, point, leaf, color, 1.25);
            draw_disc(&mut pixels, width, height, stem, 2.1, color);
        }
    }

    // Digital: regularly spaced nodes and short orthogonal-looking branches.
    let digital_strength = weights.digital.clamp(0.0, 1.0);
    if digital_strength > 0.02 {
        let node = DIGITAL.with_opacity((90.0 + 165.0 * digital_strength) / 255.0);
        for index in (6..BODY_SAMPLES).step_by(32) {
            let sample = scene
                .body_sample(index, BODY_SAMPLES)
                .map_err(OuroborosRenderError::Scene)?;
            let point = [
                center[0] + sample.centerline[0] * scale,
                center[1] + sample.centerline[1] * scale,
            ];
            let normal = normalized([point[0] - center[0], point[1] - center[1]]);
            let endpoint = [point[0] + normal[0] * 6.0, point[1] + normal[1] * 6.0];
            draw_line(&mut pixels, width, height, point, endpoint, node, 1.0);
            draw_disc(&mut pixels, width, height, endpoint, 2.2, node);
        }
    }

    // Humanity: warm cheek/temple accents on the same persistent head shape.
    let humanity_strength = weights.humanity.clamp(0.0, 1.0);
    if humanity_strength > 0.02 {
        let cheek_color = HUMANITY.with_opacity((70.0 + 155.0 * humanity_strength) / 255.0);
        for side in [-1.0_f32, 1.0] {
            let cheek = [
                anchor[0]
                    + forward[0] * head_radius * 0.55
                    + lateral[0] * side * head_radius * 0.55,
                anchor[1]
                    + forward[1] * head_radius * 0.55
                    + lateral[1] * side * head_radius * 0.55,
            ];
            draw_disc(
                &mut pixels,
                width,
                height,
                cheek,
                head_radius * 0.16,
                cheek_color,
            );
        }
    }

    // Enlightenment: a restrained set of rays around the face. It blends with
    // every other form; it is not a replacement scene or a flash.
    if halo_strength > 0.02 {
        let head = frame.head_anchor;
        let anchor = [center[0] + head[0] * scale, center[1] + head[1] * scale];
        let ray_color = ENLIGHTENMENT.with_opacity((40.0 + 95.0 * halo_strength) / 255.0);
        for index in 0..8 {
            let angle = std::f32::consts::TAU * index as f32 / 8.0;
            let direction = [angle.cos(), angle.sin()];
            let start = [
                anchor[0] + direction[0] * 12.0,
                anchor[1] + direction[1] * 12.0,
            ];
            let end = [
                anchor[0] + direction[0] * 17.0,
                anchor[1] + direction[1] * 17.0,
            ];
            draw_line(&mut pixels, width, height, start, end, ray_color, 0.9);
        }
    }

    // Persistent face: every attachment is projected from the versioned
    // semantic landmark contract. No backend-specific eye/nose/mouth offsets
    // or duplicate yaw/pitch transform are allowed.
    let head_color = Rgba::lerp(palette, HUMANITY, 0.28);
    draw_disc(&mut pixels, width, height, anchor, head_radius, head_color);
    draw_circle(
        &mut pixels,
        width,
        height,
        anchor,
        head_radius,
        48,
        ENLIGHTENMENT.with_opacity(0.55),
        1.0,
    );

    let eye_radius = head_radius * 0.16;
    let pupil_radius = eye_radius * 0.38;
    for eye in [landmarks.left_eye, landmarks.right_eye] {
        let eye = project(eye);
        draw_disc(&mut pixels, width, height, eye, eye_radius, DARK_INK);
        let pupil = [
            eye[0] + forward[0] * head_radius * 0.08,
            eye[1] + forward[1] * head_radius * 0.08,
        ];
        draw_disc(&mut pixels, width, height, pupil, pupil_radius, STARLIGHT);
    }

    let nose = project(landmarks.nose_tip);
    draw_disc(
        &mut pixels,
        width,
        height,
        nose,
        head_radius * 0.10,
        DARK_INK,
    );

    let mouth = project(landmarks.mouth_center);
    let mouth_half_width = head_radius * 0.42;
    let mouth_a = [
        mouth[0] - lateral[0] * mouth_half_width,
        mouth[1] - lateral[1] * mouth_half_width,
    ];
    let mouth_b = [
        mouth[0] + lateral[0] * mouth_half_width,
        mouth[1] + lateral[1] * mouth_half_width,
    ];
    draw_line(
        &mut pixels,
        width,
        height,
        mouth_a,
        mouth_b,
        DARK_INK.with_opacity(0.8),
        head_radius * 0.10,
    );

    Ok(pixels)
}

fn mixed_palette(weights: crate::ouroboros::FormWeights) -> Rgba {
    let sum = weights.nature + weights.humanity + weights.digital + weights.enlightenment;
    if !sum.is_finite() || sum <= f32::EPSILON {
        return NATURE;
    }

    let blend = |channel: fn(Rgba) -> u8| -> u8 {
        let value = (f32::from(channel(NATURE)) * weights.nature
            + f32::from(channel(HUMANITY)) * weights.humanity
            + f32::from(channel(DIGITAL)) * weights.digital
            + f32::from(channel(ENLIGHTENMENT)) * weights.enlightenment)
            / sum;
        value.round().clamp(0.0, 255.0) as u8
    };

    Rgba(blend(|c| c.0), blend(|c| c.1), blend(|c| c.2), 255)
}

fn fill(buffer: &mut [u8], color: Rgba) {
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[color.0, color.1, color.2, 255]);
    }
}

fn normalized(vector: [f32; 2]) -> [f32; 2] {
    let length = (vector[0] * vector[0] + vector[1] * vector[1]).sqrt();
    if length <= f32::EPSILON {
        [1.0, 0.0]
    } else {
        [vector[0] / length, vector[1] / length]
    }
}

fn blend_pixel(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Rgba) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = ((y as usize * width as usize) + x as usize) * 4;
    let destination = Rgba(
        buffer[index],
        buffer[index + 1],
        buffer[index + 2],
        buffer[index + 3],
    );
    let result = color.over(destination);
    buffer[index..index + 4].copy_from_slice(&[result.0, result.1, result.2, 255]);
}

fn draw_disc(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    center: [f32; 2],
    radius: f32,
    color: Rgba,
) {
    if !center.into_iter().all(f32::is_finite) || !radius.is_finite() || radius <= 0.0 {
        return;
    }
    let radius = radius.max(0.55);
    let min_x = (center[0] - radius - 1.0).floor() as i32;
    let max_x = (center[0] + radius + 1.0).ceil() as i32;
    let min_y = (center[1] - radius - 1.0).floor() as i32;
    let max_y = (center[1] + radius + 1.0).ceil() as i32;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - center[0];
            let dy = y as f32 + 0.5 - center[1];
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_pixel(
                    buffer,
                    width,
                    height,
                    x,
                    y,
                    color.with_opacity(f32::from(color.3) / 255.0 * coverage),
                );
            }
        }
    }
}

fn draw_line(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    start: [f32; 2],
    end: [f32; 2],
    color: Rgba,
    thickness: f32,
) {
    if !start.into_iter().chain(end).all(f32::is_finite)
        || !thickness.is_finite()
        || thickness <= 0.0
    {
        return;
    }
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let distance = (dx * dx + dy * dy).sqrt();
    let steps = distance.ceil().max(1.0) as u32;
    let radius = thickness * 0.5;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        draw_disc(
            buffer,
            width,
            height,
            [start[0] + dx * t, start[1] + dy * t],
            radius,
            color,
        );
    }
}

fn draw_circle(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    center: [f32; 2],
    radius: f32,
    segments: u32,
    color: Rgba,
    thickness: f32,
) {
    if segments < 8 || segments > 1024 || !radius.is_finite() || radius <= 0.0 {
        return;
    }
    let mut previous = [center[0] + radius, center[1]];
    for index in 1..=segments {
        let angle = std::f32::consts::TAU * index as f32 / segments as f32;
        let point = [
            center[0] + radius * angle.cos(),
            center[1] + radius * angle.sin(),
        ];
        draw_line(buffer, width, height, previous, point, color, thickness);
        previous = point;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ouroboros::{OuroborosForm, OuroborosMotionMode};

    fn scene(form: OuroborosForm) -> OuroborosScene {
        OuroborosScene::new(crate::ouroboros::OuroborosConfig::default(), form)
            .expect("default scene config is valid")
    }

    #[test]
    fn reference_artifact_receipt_binds_dimensions_and_pixel_bytes() {
        let scene = scene(OuroborosForm::Digital);
        let (left_pixels, left) = render_reference_artifact(&scene, 128, 128).unwrap();
        let (right_pixels, right) = render_reference_artifact(&scene, 128, 128).unwrap();
        assert_eq!(left, right);
        assert_eq!(left_pixels, right_pixels);
        assert_eq!(left.pixel_digest, *blake3::hash(&left_pixels).as_bytes());
        assert_eq!(
            left.scene_contract_version,
            crate::contract::SCENE_CONTRACT_VERSION
        );
        assert_ne!(
            left.pixel_digest,
            render_reference_artifact(&scene, 160, 128)
                .unwrap()
                .1
                .pixel_digest
        );
    }

    #[test]
    fn reference_output_is_opaque_and_has_exact_bounded_size() {
        let output = render_reference_rgba(&scene(OuroborosForm::Nature), 128, 96).unwrap();
        assert_eq!(output.len(), 128 * 96 * 4);
        assert!(output.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn every_semantic_form_has_a_distinct_reference_capture() {
        let hashes = OuroborosForm::ALL.map(|form| {
            let output = render_reference_rgba(&scene(form), 128, 128).unwrap();
            *blake3::hash(&output).as_bytes()
        });
        for (index, hash) in hashes.iter().enumerate() {
            assert!(
                !hashes[..index].contains(hash),
                "form {} produced a duplicate reference capture",
                OuroborosForm::ALL[index].id()
            );
        }
    }

    #[test]
    fn identical_scene_inputs_produce_byte_identical_rgba() {
        let left = scene(OuroborosForm::Digital);
        let right = scene(OuroborosForm::Digital);
        let a = render_reference_rgba(&left, 160, 160).unwrap();
        let b = render_reference_rgba(&right, 160, 160).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn reduced_mode_output_is_independent_of_time_and_attention_updates() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.set_motion_mode(OuroborosMotionMode::Reduced);
        scene.transition_to(OuroborosForm::Enlightenment);
        let first = render_reference_rgba(&scene, 128, 128).unwrap();

        scene
            .set_attention_target(crate::ouroboros::AttentionTarget::new(1.0, -1.0).unwrap())
            .unwrap();
        for _ in 0..60 {
            scene.advance(0.1).unwrap();
        }
        let second = render_reference_rgba(&scene, 128, 128).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_dimensions_are_rejected_before_allocation() {
        assert!(matches!(
            render_reference_rgba(&scene(OuroborosForm::Nature), 0, 128),
            Err(OuroborosRenderError::InvalidDimensions(
                DimensionsError::Zero
            ))
        ));
        assert!(render_reference_rgba(&scene(OuroborosForm::Nature), 4096, 4096).is_err());
    }

    #[test]
    fn reference_face_features_are_drawn_at_canonical_landmark_projections() {
        let scene = scene(OuroborosForm::Nature);
        let landmarks = scene.landmarks();
        let width = 160_u32;
        let height = 160_u32;
        let pixels = render_reference_rgba(&scene, width, height).unwrap();
        let scale = width.min(height) as f32 * 0.84;
        let project = |point: [f32; 3]| {
            [
                width as f32 * 0.5 + point[0] * scale,
                height as f32 * 0.5 + point[1] * scale,
            ]
        };
        let has_dark_ink_near = |point: [f32; 3]| {
            let screen = project(point);
            let x = screen[0].round() as i32;
            let y = screen[1].round() as i32;
            (-2..=2).any(|dy| {
                (-2..=2).any(|dx| {
                    let px = x + dx;
                    let py = y + dy;
                    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                        return false;
                    }
                    let index = ((py as u32 * width + px as u32) * 4) as usize;
                    pixels[index] == DARK_INK.0
                        && pixels[index + 1] == DARK_INK.1
                        && pixels[index + 2] == DARK_INK.2
                        && pixels[index + 3] == 255
                })
            })
        };

        assert!(has_dark_ink_near(landmarks.left_eye));
        assert!(has_dark_ink_near(landmarks.right_eye));
        assert!(has_dark_ink_near(landmarks.nose_tip));
    }

    #[test]
    fn face_pose_changes_reference_pixels_but_not_head_anchor() {
        let mut scene = scene(OuroborosForm::Humanity);
        let centered = render_reference_rgba(&scene, 160, 160).unwrap();
        scene
            .set_attention_target(crate::ouroboros::AttentionTarget::new(1.0, 0.8).unwrap())
            .unwrap();
        for _ in 0..20 {
            scene.advance(0.1).unwrap();
        }
        let turned = render_reference_rgba(&scene, 160, 160).unwrap();
        assert_ne!(centered, turned);
        assert_eq!(scene.frame().head_anchor, [0.38, 0.0, 0.0]);
    }
}
