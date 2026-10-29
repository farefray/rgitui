//! PNG decoding runs with the diff's background preparation, never in render.
use std::{io::Cursor, sync::Arc};

use gpui::prelude::*;
use gpui::{div, img, px, AnyElement, RenderImage};
use image::{Frame, ImageFormat, ImageReader, Limits};
use rgitui_git::ImageDiff;
use rgitui_i18n::t;
use rgitui_theme::{Color, StyledExt};
use rgitui_ui::{Label, LabelSize};

// Bound decoded memory as well as the encoded-byte limit in the Git layer.
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

pub(super) struct ImagePreview {
    before: PreviewSide,
    after: PreviewSide,
    error: Option<String>,
}

enum PreviewSide {
    Missing,
    Invalid,
    TooLarge,
    Ready {
        image: Arc<RenderImage>,
        width: u32,
        height: u32,
    },
}

impl ImagePreview {
    pub fn prepare(diff: &ImageDiff) -> Self {
        Self {
            before: PreviewSide::decode(diff.before.as_deref()),
            after: PreviewSide::decode(diff.after.as_deref()),
            error: diff.error.clone(),
        }
    }

    pub fn release(self, cx: &mut gpui::App) {
        // Defer until an updating window has been returned to App.windows.
        // Dropping the Arc alone does not remove directly rendered atlas images.
        cx.defer(move |cx| {
            for side in [self.before, self.after] {
                if let PreviewSide::Ready { image, .. } = side {
                    cx.drop_image(image, None);
                }
            }
        });
    }

    pub fn render(&self) -> AnyElement {
        if let Some(error) = &self.error {
            return div()
                .flex_1()
                .p(px(24.))
                .child(
                    Label::new(error.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element();
        }
        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .gap(px(12.))
            .p(px(12.))
            .child(self.before.render(t!("diff.image.before")))
            .child(self.after.render(t!("diff.image.after")))
            .into_any_element()
    }
}

#[cfg(feature = "perf")]
impl rgitui_perf::HeapSize for ImagePreview {
    fn heap_size(&self, census: &mut rgitui_perf::Census) -> usize {
        // Each side owns its decoded frame; render-time Arc clones do not
        // transfer ownership. GPU atlas memory is not part of the heap census.
        [&self.before, &self.after]
            .into_iter()
            .map(|side| match side {
                PreviewSide::Ready { image, .. } => image.as_bytes(0).map_or(0, <[u8]>::len),
                _ => 0,
            })
            .sum::<usize>()
            + rgitui_perf::HeapSize::heap_size(&self.error, census)
    }
}

impl PreviewSide {
    fn decode(bytes: Option<&[u8]>) -> Self {
        let Some(bytes) = bytes else {
            return Self::Missing;
        };
        let reader = || ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        let Ok((width, height)) = reader().into_dimensions() else {
            return Self::Invalid;
        };
        if width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            return Self::TooLarge;
        }
        let mut reader = reader();
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some(MAX_PIXELS * 8);
        reader.limits(limits);
        let Ok(decoded) = reader.decode() else {
            return Self::Invalid;
        };
        let mut rgba = decoded.into_rgba8();
        // GPUI's renderer expects BGRA, not the decoder's RGBA.
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Self::Ready {
            image: Arc::new(RenderImage::new(vec![Frame::new(rgba)])),
            width,
            height,
        }
    }

    fn render(&self, title: gpui::SharedString) -> AnyElement {
        let mut header = div()
            .h_flex()
            .gap(px(8.))
            .flex_shrink_0()
            .child(Label::new(title).size(LabelSize::Small));
        if let Self::Ready { width, height, .. } = self {
            header = header.child(
                Label::new(t!("diff.image.dimensions", width = width, height = height))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            );
        }
        let content = match self {
            Self::Ready { image, .. } => img(Arc::clone(image))
                .size_full()
                .object_fit(gpui::ObjectFit::Contain)
                .into_any_element(),
            _ => Label::new(match self {
                Self::Missing => t!("diff.image.missing"),
                Self::TooLarge => t!("diff.image.too_large"),
                _ => t!("diff.image.invalid"),
            })
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
        };
        div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap(px(8.))
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .when(matches!(self, Self::Ready { .. }), |element| {
                        element.child(checkerboard())
                    })
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(content),
                    ),
            )
            .into_any_element()
    }
}

/// A neutral transparency grid, clipped to the visible preview area.
fn checkerboard() -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let cell = px(16.);
            let columns = (bounds.size.width / cell).ceil() as usize;
            let rows = (bounds.size.height / cell).ceil() as usize;
            for row in 0..rows {
                for column in 0..columns {
                    let origin =
                        bounds.origin + gpui::point(cell * column as f32, cell * row as f32);
                    let size = gpui::size(
                        cell.min(bounds.right() - origin.x),
                        cell.min(bounds.bottom() - origin.y),
                    );
                    let color = if (row + column) % 2 == 0 {
                        0x303030
                    } else {
                        0x383838
                    };
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(origin, size),
                        gpui::rgb(color),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 128]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn decodes_png_with_dimensions() {
        assert!(matches!(
            PreviewSide::decode(Some(&png(2, 3))),
            PreviewSide::Ready {
                width: 2,
                height: 3,
                ..
            }
        ));
    }

    #[test]
    fn distinguishes_missing_and_invalid_images() {
        assert!(matches!(PreviewSide::decode(None), PreviewSide::Missing));
        assert!(matches!(
            PreviewSide::decode(Some(b"not a PNG")),
            PreviewSide::Invalid
        ));
        let bytes = png(2, 3);
        assert!(matches!(
            PreviewSide::decode(Some(&bytes[..bytes.len() / 2])),
            PreviewSide::Invalid
        ));
    }

    #[test]
    fn rejects_oversized_dimensions_before_decoding_pixels() {
        assert!(matches!(
            PreviewSide::decode(Some(&png(MAX_DIMENSION + 1, 1))),
            PreviewSide::TooLarge
        ));
    }
}
