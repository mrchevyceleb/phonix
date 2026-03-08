fn main() {
    #[cfg(windows)]
    {
        // Generate .ico file if it doesn't exist yet
        let ico_path = std::path::Path::new("assets/phonix.ico");
        if !ico_path.exists() {
            std::fs::create_dir_all("assets").unwrap();
            let ico_data = generate_ico();
            std::fs::write(ico_path, ico_data).unwrap();
        }

        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/phonix.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}

#[cfg(windows)]
fn generate_ico() -> Vec<u8> {
    let sizes = [16u32, 32, 48, 64, 256];
    let mut images: Vec<(u32, Vec<u8>)> = Vec::new();

    for &size in &sizes {
        let rgba = generate_mic_icon(100, 180, 255, size);
        let png_data = encode_png(&rgba, size);
        images.push((size, png_data));
    }

    // ICO header
    let num_images = images.len() as u16;
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&num_images.to_le_bytes());

    // Calculate offsets
    let header_size = 6 + (num_images as usize) * 16;
    let mut offset = header_size as u32;

    // Directory entries
    for (size, data) in &images {
        let s = if *size >= 256 { 0u8 } else { *size as u8 };
        ico.push(s); // width
        ico.push(s); // height
        ico.push(0); // color palette
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // color planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes()); // size
        ico.extend_from_slice(&offset.to_le_bytes()); // offset
        offset += data.len() as u32;
    }

    // Image data (PNG format for each size)
    for (_, data) in &images {
        ico.extend_from_slice(data);
    }

    ico
}

#[cfg(windows)]
fn generate_mic_icon(bg_r: u8, bg_g: u8, bg_b: u8, size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let s = size as f32 / 32.0;
    let center = size as f32 / 2.0;
    let circle_r = 13.0 * s;

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - center;
            let dy = py - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;

            let alpha = ((circle_r - dist + 0.5) * 255.0).clamp(0.0, 255.0) as u8;
            if alpha == 0 {
                continue;
            }

            let nx = dx / s;
            let ny = dy / s;

            let head_cy: f32 = -2.5;
            let head_hw: f32 = 2.5;
            let head_hh: f32 = 4.0;
            let in_head = {
                let rx = nx.abs();
                let ry = (ny - head_cy).abs();
                if ry <= head_hh - head_hw {
                    rx <= head_hw
                } else {
                    let oy = ry - (head_hh - head_hw);
                    rx * rx + oy * oy <= head_hw * head_hw
                }
            };

            let arc_cy: f32 = -0.5;
            let arc_r: f32 = 4.2;
            let arc_thick: f32 = 1.4;
            let arc_dist = (nx * nx + (ny - arc_cy).powi(2)).sqrt();
            let in_arc = (arc_dist - arc_r).abs() <= arc_thick / 2.0 && ny >= arc_cy;

            let stem_top = arc_cy + arc_r;
            let stem_bottom = stem_top + 2.5;
            let in_stem = nx.abs() <= 0.7 && ny >= stem_top && ny <= stem_bottom;

            let in_base = nx.abs() <= 2.8 && (ny - stem_bottom).abs() <= 0.7;

            if in_head || in_arc || in_stem || in_base {
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
                rgba[i + 3] = alpha;
            } else {
                rgba[i] = bg_r;
                rgba[i + 1] = bg_g;
                rgba[i + 2] = bg_b;
                rgba[i + 3] = alpha;
            }
        }
    }
    rgba
}

#[cfg(windows)]
fn encode_png(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut png = Vec::new();

    // PNG signature
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // IHDR chunk
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes()); // width
    ihdr.extend_from_slice(&size.to_be_bytes()); // height
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type: RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    write_chunk(&mut png, b"IHDR", &ihdr);

    // IDAT chunk - uncompressed deflate
    let mut raw = Vec::new();
    for y in 0..size {
        raw.push(0); // filter: none
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            raw.extend_from_slice(&rgba[i..i + 4]);
        }
    }

    // Wrap in zlib/deflate format
    let compressed = deflate_uncompressed(&raw);
    write_chunk(&mut png, b"IDAT", &compressed);

    // IEND
    write_chunk(&mut png, b"IEND", &[]);

    png
}

#[cfg(windows)]
fn write_chunk(png: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(chunk_type);
    png.extend_from_slice(data);
    let mut crc_data = Vec::new();
    crc_data.extend_from_slice(chunk_type);
    crc_data.extend_from_slice(data);
    let crc = crc32(&crc_data);
    png.extend_from_slice(&crc.to_be_bytes());
}

#[cfg(windows)]
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(windows)]
fn deflate_uncompressed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // zlib header
    out.push(0x78); // CMF
    out.push(0x01); // FLG

    // Split into 65535-byte blocks
    let chunks: Vec<&[u8]> = data.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i == chunks.len() - 1;
        out.push(if is_last { 1 } else { 0 }); // BFINAL + BTYPE=00 (uncompressed)
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }

    // Adler-32 checksum
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());

    out
}

#[cfg(windows)]
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
