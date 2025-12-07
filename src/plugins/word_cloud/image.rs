use super::stopwords::get_stop_words;
use crate::info;
use araea_wordcloud::{WordCloudBuilder, WordInput};
use base64::{Engine as _, engine::general_purpose};
use image::{GenericImageView, ImageFormat};
use rand::Rng;
use std::collections::HashMap;
use std::io::Cursor;
use std::time::Instant;

pub fn generate_word_cloud(
    corpus: Vec<String>,
    font_path: Option<String>,
    limit: usize,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let start = Instant::now();

    let stop_words = get_stop_words();
    let mut freq_map: HashMap<String, f64> = HashMap::new();

    for line in corpus {
        let words = line.split_whitespace();
        for w in words {
            let w_trim = w.trim();
            // 过滤规则：长度>1，不在停用词表，非纯数字
            if w_trim.chars().count() > 1
                && !stop_words.contains(w_trim)
                && !w_trim
                    .chars()
                    .all(|c| c.is_numeric() || c.is_ascii_punctuation())
            {
                *freq_map.entry(w_trim.to_string()).or_insert(0.0) += 1.0;
            }
        }
    }

    if freq_map.is_empty() {
        return Err("有效词汇为空（可能被过滤）".to_string());
    }

    let mut word_vec: Vec<(String, f64)> = freq_map.into_iter().collect();
    word_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let top_words: Vec<WordInput> = word_vec
        .into_iter()
        .take(limit)
        .map(|(text, size)| WordInput::new(text, size as f32))
        .collect();

    let mut rng = rand::rng();
    let mut builder = WordCloudBuilder::new()
        .size(width, height)
        .seed(rng.random());

    if let Some(path) = font_path {
        match std::fs::read(&path) {
            Ok(font_data) => {
                builder = builder.font(font_data);
            }
            Err(e) => {
                return Err(format!("加载字体文件失败: {} - {}", path, e));
            }
        }
    }

    let wordcloud = builder
        .angles(vec![0.0, 90.0])
        .vertical_writing(true)
        .build(&top_words)
        .map_err(|e| format!("Build Error: {}", e))?;

    let png_data = wordcloud
        .to_png(2.0)
        .map_err(|e| format!("PNG Encode Error: {}", e))?;

    // 自动裁剪
    let img = image::load_from_memory(&png_data).map_err(|e| format!("Image Load Error: {}", e))?;

    let (img_w, img_h) = img.dimensions();
    let mut min_x = img_w;
    let mut min_y = img_h;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found_content = false;

    // 扫描非白像素
    for y in 0..img_h {
        for x in 0..img_w {
            let pixel = img.get_pixel(x, y);
            // 假设背景纯白 (255, 255, 255)，允许少量误差
            if pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250 {
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
                found_content = true;
            }
        }
    }

    let final_data = if found_content {
        let padding = 20;
        let crop_min_x = min_x.saturating_sub(padding);
        let crop_min_y = min_y.saturating_sub(padding);
        let crop_max_x = (max_x + padding).min(img_w - 1);
        let crop_max_y = (max_y + padding).min(img_h - 1);

        let crop_width = crop_max_x - crop_min_x + 1;
        let crop_height = crop_max_y - crop_min_y + 1;

        let cropped_img = img.crop_imm(crop_min_x, crop_min_y, crop_width, crop_height);

        let mut buffer = Cursor::new(Vec::new());
        cropped_img
            .write_to(&mut buffer, ImageFormat::Png)
            .map_err(|e| format!("Image Write Error: {}", e))?;
        buffer.into_inner()
    } else {
        png_data
    };

    let b64_str = general_purpose::STANDARD.encode(&final_data);

    info!(target: "Plugin/WordCloud", "Generated & Cropped in {:?}", start.elapsed());

    Ok(format!("base64://{}", b64_str))
}
