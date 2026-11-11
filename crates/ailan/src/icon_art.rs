//! Pure software SDF renderer for the AiLan application and tray icons.
//!
//! Generates a modern monochrome icon:
//! - Deep black rounded squircle container (#0A0A0C) with subtle hairline border.
//! - Smooth white pill (the Dynamic Island notch) positioned in the upper portion.
//! - Elegant 4-pointed sparkle (✦) representing "Ai" in the lower right.

/// Render BGRA premultiplied 32-bit pixel buffer for an icon of dimensions `size × size`.
pub fn render_icon(size: i32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];

    // Squircle container parameters
    let corner_rad = s * 0.24;
    let inset = (s * 0.04).max(0.5);
    let box_half_w = s * 0.5 - inset;
    let box_half_h = s * 0.5 - inset;

    // Pill (Dynamic Island) parameters
    let (pw, ph) = (s * 0.54, s * 0.22);
    let (pcx, pcy) = (s * 0.5, s * 0.38);
    let pr = ph * 0.5;

    // Sparkle (Ai) parameters - visible on icons >= 24px
    let show_sparkle = size >= 24;
    let (scx, scy) = (s * 0.68, s * 0.70);
    let s_radius = s * 0.16;

    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            // 1. Distance to squircle container
            let qx = (fx - s * 0.5).abs() - (box_half_w - corner_rad);
            let qy = (fy - s * 0.5).abs() - (box_half_h - corner_rad);
            let d_box = if qx > 0.0 && qy > 0.0 {
                (qx * qx + qy * qy).sqrt() - corner_rad
            } else {
                qx.max(qy) - corner_rad
            };
            let body_mask = (0.5 - d_box).clamp(0.0, 1.0);

            if body_mask <= 0.001 {
                continue;
            }

            // 2. Subtle 1px inner border highlight
            let border_dist = d_box.abs();
            let border_mask = (1.0 - border_dist).clamp(0.0, 1.0);

            // 3. Distance to white pill
            let pqx = (fx - pcx).abs() - (pw * 0.5 - pr);
            let pqy = (fy - pcy).abs();
            let d_pill = if pqx > 0.0 {
                (pqx * pqx + pqy * pqy).sqrt() - pr
            } else {
                pqy - pr
            };
            let pill_mask = (0.5 - d_pill).clamp(0.0, 1.0);

            // 4. Sparkle (Ai) shape: intersection of 4 circular cutouts or diamond curve
            let mut sparkle_mask = 0.0f32;
            if show_sparkle {
                let dx = (fx - scx).abs();
                let dy = (fy - scy).abs();
                if dx < s_radius && dy < s_radius {
                    // Astroid / 4-pointed star curve: (x/r)^0.5 + (y/r)^0.5 <= 1
                    let norm_x = dx / s_radius;
                    let norm_y = dy / s_radius;
                    let val = norm_x.sqrt() + norm_y.sqrt();
                    let d_star = (val - 1.0) * s_radius;
                    sparkle_mask = (0.5 - d_star).clamp(0.0, 1.0);
                }
            }

            // Combine layers
            let bg_val = 14.0 * body_mask;
            let border_val = 255.0 * (border_mask * 0.16 * body_mask);
            let fg_mask = (pill_mask + sparkle_mask).clamp(0.0, 1.0);
            let fg_val = 255.0 * fg_mask;

            let final_rgb = ((bg_val + border_val) * (1.0 - fg_mask) + fg_val).clamp(0.0, 255.0) as u8;
            let final_alpha = ((body_mask.max(fg_mask)) * 255.0).clamp(0.0, 255.0) as u8;

            let idx = ((y * size + x) * 4) as usize;
            px[idx] = final_rgb;     // B
            px[idx + 1] = final_rgb; // G
            px[idx + 2] = final_rgb; // R
            px[idx + 3] = final_alpha; // A
        }
    }
    px
}

/// Generate valid .ico binary file bytes containing multiple icon sizes.
pub fn render_ico_bytes(sizes: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();

    // 1. ICO Header (6 bytes)
    out.extend_from_slice(&0u16.to_le_bytes()); // Reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // Type = 1 (Icon)
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes()); // Number of images

    let mut images_data: Vec<Vec<u8>> = Vec::new();
    let header_size = 6 + sizes.len() * 16;
    let mut current_offset = header_size as u32;

    for &size in sizes {
        let pixels = render_icon(size);
        let mask_row_bytes = ((size + 31) / 32) * 4;
        let mask_bytes = (mask_row_bytes * size) as usize;
        let dib_header_size = 40u32;
        let image_bytes = dib_header_size + (pixels.len() as u32) + (mask_bytes as u32);

        // Directory entry (16 bytes)
        out.push(if size >= 256 { 0 } else { size as u8 }); // Width
        out.push(if size >= 256 { 0 } else { size as u8 }); // Height
        out.push(0); // Color count
        out.push(0); // Reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // Color planes
        out.extend_from_slice(&32u16.to_le_bytes()); // Bits per pixel
        out.extend_from_slice(&image_bytes.to_le_bytes()); // Size of image data
        out.extend_from_slice(&current_offset.to_le_bytes()); // Offset of image data

        current_offset += image_bytes;

        // Prepare DIB image payload (BITMAPINFOHEADER + bottom-up pixels + 1-bit mask)
        let mut dib = Vec::with_capacity(image_bytes as usize);
        dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
        dib.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
        dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight (double for XOR + AND mask)
        dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
        dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB)
        dib.extend_from_slice(&((pixels.len() + mask_bytes) as u32).to_le_bytes()); // biSizeImage
        dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
        dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
        dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
        dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

        // Bottom-up BGRA rows
        for y in (0..size).rev() {
            let start = (y * size * 4) as usize;
            let end = start + (size * 4) as usize;
            dib.extend_from_slice(&pixels[start..end]);
        }

        // AND mask (all zeros for 32-bit alpha icons)
        dib.resize(dib.len() + mask_bytes, 0);

        images_data.push(dib);
    }

    // Append image data
    for img in images_data {
        out.extend_from_slice(&img);
    }

    out
}
