use anyhow::Result;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Read Stable Diffusion `parameters` text from PNG bytes in memory.
pub fn extract_parameters(png_bytes: &[u8]) -> Result<Option<String>> {
    if !png_bytes.starts_with(PNG_SIGNATURE) {
        return Ok(None);
    }

    let mut offset = PNG_SIGNATURE.len();

    while offset + 8 <= png_bytes.len() {
        let length = u32::from_be_bytes(
            png_bytes[offset..offset + 4]
                .try_into()
                .expect("chunk length slice"),
        ) as usize;
        let chunk_type = &png_bytes[offset + 4..offset + 8];
        offset += 8;

        if offset + length + 4 > png_bytes.len() {
            break;
        }

        let data = &png_bytes[offset..offset + length];

        if chunk_type == b"tEXt" {
            if let Some(text) = parse_text_chunk(data, "parameters") {
                return Ok(Some(text));
            }
        } else if chunk_type == b"iTXt" {
            if let Some(text) = parse_itxt_chunk(data, "parameters") {
                return Ok(Some(text));
            }
        } else if chunk_type == b"IEND" {
            break;
        }

        offset += length + 4;
    }

    Ok(None)
}

fn parse_text_chunk(data: &[u8], keyword: &str) -> Option<String> {
    let null_pos = data.iter().position(|byte| *byte == 0)?;
    let chunk_keyword = std::str::from_utf8(&data[..null_pos]).ok()?;
    if chunk_keyword != keyword {
        return None;
    }
    std::str::from_utf8(&data[null_pos + 1..])
        .ok()
        .map(str::to_string)
}

fn parse_itxt_chunk(data: &[u8], keyword: &str) -> Option<String> {
    let mut cursor = 0;
    let keyword_end = data[cursor..].iter().position(|byte| *byte == 0)?;
    let chunk_keyword = std::str::from_utf8(&data[cursor..cursor + keyword_end]).ok()?;
    if chunk_keyword != keyword {
        return None;
    }
    cursor += keyword_end + 1;

    if cursor + 2 > data.len() {
        return None;
    }
    let compression_flag = data[cursor];
    cursor += 2; // compression flag + compression method

    while cursor < data.len() && data[cursor] != 0 {
        cursor += 1;
    }
    if cursor >= data.len() {
        return None;
    }
    cursor += 1; // language tag null terminator

    while cursor < data.len() && data[cursor] != 0 {
        cursor += 1;
    }
    if cursor >= data.len() {
        return None;
    }
    cursor += 1; // translated keyword null terminator

    let text_bytes = &data[cursor..];
    if compression_flag == 0 {
        return std::str::from_utf8(text_bytes).ok().map(str::to_string);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use png::{BitDepth, ColorType};

    fn png_with_parameters(text: &str) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, 1, 1);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            encoder
                .add_text_chunk("parameters".to_string(), text.to_string())
                .unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 0, 0, 0]).unwrap();
        }
        output
    }

    #[test]
    fn reads_parameters_text_chunk() {
        let png = png_with_parameters("1girl, masterpiece");
        let params = extract_parameters(&png).unwrap();
        assert_eq!(params.as_deref(), Some("1girl, masterpiece"));
    }

    #[test]
    fn returns_none_when_missing() {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, 1, 1);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 0, 0, 0]).unwrap();
        }
        assert!(extract_parameters(&output).unwrap().is_none());
    }
}
