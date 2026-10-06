//! Blend modes.
//!
//! This module is the single definition of what each mode computes. The
//! compositor (CPU now, GPU later) is the only caller.
//!
//! [`blend_rgb`] takes and returns straight (non-premultiplied) colour in
//! `0.0..=1.0`, in the document's working space. 8-bit and 16-bit documents
//! are gamma-encoded, so their blends are computed on encoded values, as
//! Photoshop does by default. See `docs/BLEND_MODES.md` for the formulas.

/// The 27 layer blend modes, in the conventional menu order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 27] = [
        BlendMode::Normal,
        BlendMode::Dissolve,
        BlendMode::Darken,
        BlendMode::Multiply,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::DarkerColor,
        BlendMode::Lighten,
        BlendMode::Screen,
        BlendMode::ColorDodge,
        BlendMode::LinearDodge,
        BlendMode::LighterColor,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Subtract,
        BlendMode::Divide,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// Display name.
    pub const fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Dissolve => "Dissolve",
            BlendMode::Darken => "Darken",
            BlendMode::Multiply => "Multiply",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::DarkerColor => "Darker Color",
            BlendMode::Lighten => "Lighten",
            BlendMode::Screen => "Screen",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::LinearDodge => "Linear Dodge",
            BlendMode::LighterColor => "Lighter Color",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::VividLight => "Vivid Light",
            BlendMode::LinearLight => "Linear Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::HardMix => "Hard Mix",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        }
    }
}

impl BlendMode {
    /// A stable machine name (`"color_burn"`), used in saved files and by
    /// anything else that refers to a mode by name. Never change one.
    pub fn id(self) -> String {
        self.name().to_lowercase().replace(' ', "_")
    }

    /// Inverse of [`BlendMode::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }
}

/// The blend function `B(backdrop, source)` on straight colour in `0..=1`.
///
/// Dissolve has no colour function of its own: it is Normal with a
/// per-pixel coverage decision, which the compositor makes.
#[inline]
pub fn blend_rgb(mode: BlendMode, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    let per_channel = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
    match mode {
        BlendMode::Normal | BlendMode::Dissolve => s,
        BlendMode::Darken => per_channel(f32::min),
        BlendMode::Multiply => per_channel(|b, s| b * s),
        BlendMode::ColorBurn => per_channel(color_burn),
        BlendMode::LinearBurn => per_channel(|b, s| (b + s - 1.0).max(0.0)),
        BlendMode::DarkerColor => {
            if lum(s) < lum(b) {
                s
            } else {
                b
            }
        }
        BlendMode::Lighten => per_channel(f32::max),
        BlendMode::Screen => per_channel(screen),
        BlendMode::ColorDodge => per_channel(color_dodge),
        BlendMode::LinearDodge => per_channel(|b, s| (b + s).min(1.0)),
        BlendMode::LighterColor => {
            if lum(s) > lum(b) {
                s
            } else {
                b
            }
        }
        BlendMode::Overlay => per_channel(|b, s| hard_light(s, b)),
        BlendMode::SoftLight => per_channel(soft_light),
        BlendMode::HardLight => per_channel(hard_light),
        BlendMode::VividLight => per_channel(|b, s| {
            if s <= 0.5 {
                color_burn(b, 2.0 * s)
            } else {
                color_dodge(b, 2.0 * s - 1.0)
            }
        }),
        BlendMode::LinearLight => per_channel(|b, s| (b + 2.0 * s - 1.0).clamp(0.0, 1.0)),
        BlendMode::PinLight => per_channel(|b, s| {
            if s <= 0.5 {
                b.min(2.0 * s)
            } else {
                b.max(2.0 * s - 1.0)
            }
        }),
        BlendMode::HardMix => per_channel(|b, s| if b + s >= 1.0 { 1.0 } else { 0.0 }),
        BlendMode::Difference => per_channel(|b, s| (b - s).abs()),
        BlendMode::Exclusion => per_channel(|b, s| b + s - 2.0 * b * s),
        BlendMode::Subtract => per_channel(|b, s| (b - s).max(0.0)),
        BlendMode::Divide => per_channel(|b, s| {
            if b <= 0.0 {
                0.0
            } else if s <= 0.0 {
                1.0
            } else {
                (b / s).min(1.0)
            }
        }),
        BlendMode::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        BlendMode::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        BlendMode::Color => set_lum(s, lum(b)),
        BlendMode::Luminosity => set_lum(b, lum(s)),
    }
}

#[inline]
fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

#[inline]
fn color_burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

#[inline]
fn color_dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

#[inline]
fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        screen(b, 2.0 * s - 1.0)
    }
}

/// Soft Light as defined by the PDF and W3C compositing specifications.
#[inline]
fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 {
            ((16.0 * b - 12.0) * b + 4.0) * b
        } else {
            b.sqrt()
        };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

// Non-separable helpers, as defined by the PDF and W3C specifications.

#[inline]
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

#[inline]
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

#[inline]
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        out = out.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    out
}

#[inline]
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

/// Rescales `c` so its saturation is `s`, keeping the channel ordering.
#[inline]
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max > min {
        // (v - min) / (max - min) is 0 for the smallest channel, 1 for the
        // largest and proportional for the middle one.
        c.map(|v| (v - min) * s / (max - min))
    } else {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BlendMode::*;

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{what}: got {actual}, expected {expected}"
        );
    }

    /// Runs a separable mode on a grey pair and returns one channel.
    fn sep(mode: BlendMode, b: f32, s: f32) -> f32 {
        let out = blend_rgb(mode, [b; 3], [s; 3]);
        assert_eq!(out[0], out[1]);
        assert_eq!(out[1], out[2]);
        out[0]
    }

    /// Expected values worked by hand from each mode's definition, not
    /// produced by this code.
    #[test]
    fn separable_modes_match_hand_computed_values() {
        // (mode, backdrop, source, expected)
        let cases: &[(BlendMode, f32, f32, f32)] = &[
            (Normal, 0.4, 0.6, 0.6),
            (Darken, 0.4, 0.6, 0.4),
            (Lighten, 0.4, 0.6, 0.6),
            (Multiply, 0.4, 0.6, 0.24),
            (Multiply, 0.8, 0.3, 0.24),
            (Screen, 0.4, 0.6, 0.76),
            (Screen, 0.8, 0.3, 0.86),
            (ColorBurn, 0.4, 0.6, 0.0),
            (ColorBurn, 0.8, 0.3, 1.0 / 3.0),
            (LinearBurn, 0.8, 0.3, 0.1),
            (LinearBurn, 0.3, 0.3, 0.0),
            (ColorDodge, 0.2, 0.5, 0.4),
            (ColorDodge, 0.8, 0.3, 1.0),
            (LinearDodge, 0.2, 0.5, 0.7),
            (LinearDodge, 0.8, 0.6, 1.0),
            (Overlay, 0.4, 0.6, 0.48),
            (Overlay, 0.8, 0.3, 0.72),
            (SoftLight, 0.8, 0.3, 0.736),
            (SoftLight, 0.4, 0.6, 0.446_491_1),
            (SoftLight, 0.2, 0.75, 0.324),
            (HardLight, 0.4, 0.6, 0.52),
            (HardLight, 0.8, 0.3, 0.48),
            (VividLight, 0.4, 0.6, 0.5),
            (VividLight, 0.8, 0.3, 2.0 / 3.0),
            (LinearLight, 0.4, 0.6, 0.6),
            (LinearLight, 0.8, 0.3, 0.4),
            (LinearLight, 0.1, 0.2, 0.0),
            (PinLight, 0.4, 0.6, 0.4),
            (PinLight, 0.8, 0.3, 0.6),
            (HardMix, 0.3, 0.6, 0.0),
            (HardMix, 0.5, 0.7, 1.0),
            (Difference, 0.4, 0.6, 0.2),
            (Difference, 0.8, 0.3, 0.5),
            (Exclusion, 0.4, 0.6, 0.52),
            (Exclusion, 0.8, 0.3, 0.62),
            (Subtract, 0.4, 0.6, 0.0),
            (Subtract, 0.8, 0.3, 0.5),
            (Divide, 0.4, 0.6, 2.0 / 3.0),
            (Divide, 0.8, 0.3, 1.0),
        ];
        for &(mode, b, s, expected) in cases {
            assert_close(sep(mode, b, s), expected, &format!("{mode:?}({b}, {s})"));
        }
    }

    #[test]
    fn division_edge_cases_are_defined() {
        assert_eq!(sep(ColorBurn, 1.0, 0.0), 1.0);
        assert_eq!(sep(ColorBurn, 0.5, 0.0), 0.0);
        assert_eq!(sep(ColorDodge, 0.0, 1.0), 0.0);
        assert_eq!(sep(ColorDodge, 0.5, 1.0), 1.0);
        assert_eq!(sep(Divide, 0.0, 0.0), 0.0);
        assert_eq!(sep(Divide, 0.5, 0.0), 1.0);
        assert_eq!(sep(VividLight, 0.5, 0.0), 0.0);
        assert_eq!(sep(VividLight, 0.5, 1.0), 1.0);
    }

    #[test]
    fn neutral_source_colours_leave_the_backdrop_unchanged() {
        for i in 0..=20 {
            let b = i as f32 / 20.0;
            for (mode, neutral) in [
                (Multiply, 1.0),
                (Darken, 1.0),
                (ColorBurn, 1.0),
                (LinearBurn, 1.0),
                (Divide, 1.0),
                (Screen, 0.0),
                (Lighten, 0.0),
                (ColorDodge, 0.0),
                (LinearDodge, 0.0),
                (Difference, 0.0),
                (Exclusion, 0.0),
                (Subtract, 0.0),
                (Overlay, 0.5),
                (SoftLight, 0.5),
                (HardLight, 0.5),
                (VividLight, 0.5),
                (LinearLight, 0.5),
                (PinLight, 0.5),
            ] {
                assert_close(
                    sep(mode, b, neutral),
                    b,
                    &format!("{mode:?} with neutral {neutral}"),
                );
            }
        }
    }

    #[test]
    fn commutative_modes_commute() {
        for mode in [
            Darken,
            Multiply,
            LinearBurn,
            Lighten,
            Screen,
            LinearDodge,
            Difference,
            Exclusion,
            HardMix,
        ] {
            for (b, s) in [(0.1, 0.9), (0.33, 0.5), (0.7, 0.2)] {
                assert_close(sep(mode, b, s), sep(mode, s, b), &format!("{mode:?}"));
            }
        }
    }

    #[test]
    fn overlay_is_hard_light_with_layers_swapped() {
        for (b, s) in [(0.1, 0.9), (0.33, 0.5), (0.7, 0.2), (0.6, 0.6)] {
            assert_close(
                sep(Overlay, b, s),
                sep(HardLight, s, b),
                "overlay/hard light",
            );
        }
    }

    #[test]
    fn every_mode_stays_in_range_on_a_grid() {
        let steps = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0];
        for mode in BlendMode::ALL {
            for &br in &steps {
                for &bg in &steps {
                    for &sr in &steps {
                        for &sg in &steps {
                            let out = blend_rgb(mode, [br, bg, 0.3], [sr, sg, 0.8]);
                            for v in out {
                                assert!(
                                    (-1e-6..=1.0 + 1e-6).contains(&v) && v.is_finite(),
                                    "{mode:?} produced {v}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn assert_rgb(actual: [f32; 3], expected: [f32; 3], what: &str) {
        for i in 0..3 {
            assert_close(actual[i], expected[i], what);
        }
    }

    /// Hand-computed with Lum = 0.3 R + 0.59 G + 0.11 B.
    #[test]
    fn non_separable_modes_match_hand_computed_values() {
        let b = [0.2, 0.4, 0.6]; // Lum 0.362, Sat 0.4
        let grey = [0.5, 0.5, 0.5];

        // A grey source has no hue or saturation to give.
        assert_rgb(
            blend_rgb(Color, b, grey),
            [0.362; 3],
            "Color with grey source",
        );
        assert_rgb(blend_rgb(Hue, b, grey), [0.362; 3], "Hue with grey source");
        assert_rgb(
            blend_rgb(Saturation, b, grey),
            [0.362; 3],
            "Saturation with grey source",
        );
        // Luminosity keeps the backdrop's colour and takes the source's
        // lightness: shift by 0.5 - 0.362.
        assert_rgb(
            blend_rgb(Luminosity, b, grey),
            [0.338, 0.538, 0.738],
            "Luminosity",
        );

        // Pure red at the luminance of pure green needs clipping:
        // (1.29, 0.29, 0.29) is pulled toward its luminance 0.59.
        assert_rgb(
            blend_rgb(Luminosity, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            [1.0, 0.414_285_7, 0.414_285_7],
            "Luminosity with clipping",
        );

        // Hue of pure red, saturation and luminance of the backdrop:
        // SetSat(red, 0.4) = (0.4, 0, 0), Lum 0.12, shift by 0.242.
        assert_rgb(
            blend_rgb(Hue, b, [1.0, 0.0, 0.0]),
            [0.642, 0.242, 0.242],
            "Hue",
        );

        // Backdrop hue, source saturation 1.0: SetSat(b, 1) = (0, 0.5, 1),
        // Lum 0.405, shift by -0.043 gives (-0.043, 0.457, 0.957), then the
        // negative channel is clipped toward Lum 0.362.
        let k = 0.362 / 0.405;
        assert_rgb(
            blend_rgb(Saturation, b, [1.0, 0.0, 0.0]),
            [
                0.0,
                0.362 + (0.457 - 0.362) * k,
                0.362 + (0.957 - 0.362) * k,
            ],
            "Saturation",
        );
    }

    #[test]
    fn darker_and_lighter_color_pick_whole_colours() {
        let b = [0.2, 0.4, 0.6]; // Lum 0.362
        let s = [0.9, 0.1, 0.1]; // Lum 0.34
        assert_eq!(blend_rgb(DarkerColor, b, s), s);
        assert_eq!(blend_rgb(LighterColor, b, s), b);
        assert_eq!(blend_rgb(DarkerColor, s, b), s);
        assert_eq!(blend_rgb(LighterColor, s, b), b);
    }

    /// These names are written into saved documents.
    #[test]
    fn ids_are_stable_and_round_trip() {
        let ids: Vec<String> = BlendMode::ALL.iter().map(|m| m.id()).collect();
        assert_eq!(
            ids.join(" "),
            "normal dissolve darken multiply color_burn linear_burn darker_color lighten screen \
             color_dodge linear_dodge lighter_color overlay soft_light hard_light vivid_light \
             linear_light pin_light hard_mix difference exclusion subtract divide hue saturation \
             color luminosity"
        );
        for mode in BlendMode::ALL {
            assert_eq!(BlendMode::from_id(&mode.id()), Some(mode));
        }
        assert_eq!(BlendMode::from_id("Normal"), None);
        assert_eq!(BlendMode::from_id(""), None);
    }

    #[test]
    fn there_are_27_distinct_named_modes() {
        let names: std::collections::HashSet<_> = BlendMode::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(names.len(), 27);
    }
}
