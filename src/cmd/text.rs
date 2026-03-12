use crate::util::resolve_resources;
use lopdf::{Document, content::Content};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct TextRun {
    pub x: f32,
    pub y: f32,
    pub font_name: String,
    pub font_size: f32,
    pub text: String,
}

// ──────────────────────────────────────────────
// ToUnicode CMap 解析
// ──────────────────────────────────────────────

/// 从 ToUnicode 流数据中构建 GID→char 映射表
pub(crate) fn parse_to_unicode(data: &[u8]) -> HashMap<u16, String> {
    let mut map = HashMap::new();
    let text = String::from_utf8_lossy(data);

    // 解析 beginbfchar ... endbfchar
    for block in text.split("beginbfchar") {
        let end = match block.find("endbfchar") {
            Some(e) => e,
            None => continue,
        };
        for line in block[..end].lines() {
            let line = line.trim();
            if line.starts_with('<') {
                // 格式: <src_hex> <dst_hex>
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let src = parse_hex_token(parts[0]);
                    let dst = decode_hex_unicode(parts[1]);
                    if let (Some(s), Some(d)) = (src, dst) {
                        map.insert(s, d);
                    }
                }
            }
        }
    }

    // 解析 beginbfrange ... endbfrange
    for block in text.split("beginbfrange") {
        let end = match block.find("endbfrange") {
            Some(e) => e,
            None => continue,
        };
        for line in block[..end].lines() {
            let line = line.trim();
            if line.starts_with('<') {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let lo = parse_hex_token(parts[0]);
                    let hi = parse_hex_token(parts[1]);
                    if let (Some(l), Some(h)) = (lo, hi) {
                        // 第三个 token 可能是 <hex> 或 [ ... ]
                        if parts[2].starts_with('<')
                            && let Some(base_char) = decode_hex_unicode(parts[2])
                            && let Some(base_cp) = base_char.chars().next()
                        {
                            let base_cp = base_cp as u32;
                            for (i, code) in (l..=h).enumerate() {
                                if let Some(c) = char::from_u32(base_cp + i as u32) {
                                    map.insert(code, c.to_string());
                                }
                            }
                        }
                        // 数组形式 [ <h1> <h2> ... ] 略去（罕见）
                    }
                }
            }
        }
    }

    map
}

pub(crate) fn parse_hex_token(s: &str) -> Option<u16> {
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    u16::from_str_radix(inner, 16).ok()
}

pub(crate) fn decode_hex_unicode(s: &str) -> Option<String> {
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    if inner.len() >= 4 && inner.len() % 4 == 0 {
        let codepoints: Vec<u16> = (0..inner.len())
            .step_by(4)
            .filter_map(|i| u16::from_str_radix(&inner[i..i + 4], 16).ok())
            .collect();
        Some(String::from_utf16_lossy(&codepoints))
    } else if inner.len() == 2 {
        let cp = u16::from_str_radix(inner, 16).ok()?;
        char::from_u32(cp as u32).map(|c| c.to_string())
    } else {
        // 其他奇数长度或非标准长度，尽可能解析
        let bytes = (0..inner.len())
            .step_by(2)
            .filter_map(|i| {
                let end = (i + 2).min(inner.len());
                u8::from_str_radix(&inner[i..end], 16).ok()
            })
            .collect::<Vec<u8>>();
        Some(String::from_utf8_lossy(&bytes).to_string())
    }
}

/// 利用 CMap 把原始字节串解码为可读字符串
pub(crate) fn decode_bytes_with_cmap(bytes: &[u8], cmap: Option<&HashMap<u16, String>>) -> String {
    let Some(map) = cmap else {
        // 如果没有 CMap，尝试 GBK 回退
        return decode_fallback(bytes);
    };
    if map.is_empty() {
        return decode_fallback(bytes);
    }

    // 尝试 2 字节解码
    let mut decoded_2byte = String::new();
    let mut q2 = 0;
    let mut j = 0;
    while j + 1 < bytes.len() {
        let gid = ((bytes[j] as u16) << 8) | bytes[j + 1] as u16;
        if let Some(s) = map.get(&gid) {
            decoded_2byte.push_str(s);
        } else {
            decoded_2byte.push('?');
            q2 += 1;
        }
        j += 2;
    }
    if j < bytes.len() {
        let gid = bytes[j] as u16;
        if let Some(s) = map.get(&gid) {
            decoded_2byte.push_str(s);
        } else {
            decoded_2byte.push('?');
            q2 += 1;
        }
    }

    // 尝试 1 字节解码
    let mut decoded_1byte = String::new();
    let mut q1 = 0;
    for &b in bytes {
        if let Some(s) = map.get(&(b as u16)) {
            decoded_1byte.push_str(s);
        } else {
            decoded_1byte.push('?');
            q1 += 1;
        }
    }

    let total2 = bytes.len().div_ceil(2);
    let total1 = bytes.len();
    let err_rate2 = q2 as f32 / total2.max(1) as f32;
    let err_rate1 = q1 as f32 / total1.max(1) as f32;

    // 如果两者错误率都太高 (> 80%)，很有可能是编码错乱，尝试 GBK
    if err_rate2 > 0.8 && err_rate1 > 0.8 {
        return decode_fallback(bytes);
    }

    if err_rate2 <= err_rate1 {
        decoded_2byte
    } else {
        decoded_1byte
    }
}

pub(crate) fn decode_fallback(bytes: &[u8]) -> String {
    // 1. 如果全是 ASCII，直接返回
    if bytes.iter().all(|&b| (32..=126).contains(&b)) {
        return String::from_utf8_lossy(bytes).to_string();
    }
    // 2. 尝试 UTF-8
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // 3. 只有当字节数较多且包含大量高位字节时，才尝试 GBK
    if bytes.len() >= 2 {
        let high_byte_count = bytes.iter().filter(|&&b| b > 127).count();
        if high_byte_count as f32 / bytes.len() as f32 > 0.3 {
            let (res, _, has_error) = encoding_rs::GBK.decode(bytes);
            if !has_error {
                return res.into_owned();
            }
        }
    }
    // 4. 实在不行，逐字节处理：可见字符原样输出，不可见字符过滤
    bytes
        .iter()
        .filter_map(|&b| {
            if (32..=126).contains(&b) {
                Some(b as char)
            } else {
                None
            }
        })
        .collect()
}

/// 为页面的每个字体构建 CMap（resourceName → HashMap<u16,String>）
pub(crate) fn build_cmap_table(
    doc: &Document,
    page_id: lopdf::ObjectId,
) -> HashMap<String, HashMap<u16, String>> {
    let mut table = HashMap::new();
    let Ok(resources) = crate::util::resolve_resources(doc, page_id) else {
        return table;
    };
    let font_dict = match resources.get(b"Font") {
        Ok(f) => {
            if let Ok(r) = f.as_reference() {
                doc.get_object(r)
                    .and_then(|o| o.as_dict())
                    .cloned()
                    .unwrap_or_default()
            } else {
                f.as_dict().cloned().unwrap_or_default()
            }
        }
        Err(_) => return table,
    };

    for (font_key, font_ref) in &font_dict {
        let fname = String::from_utf8_lossy(font_key).to_string();
        let fid = match font_ref.as_reference() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let font_obj = match doc.get_object(fid).and_then(|o| o.as_dict()) {
            Ok(d) => d,
            Err(_) => continue,
        };
        // 获取 ToUnicode 流
        let to_u_id = match font_obj.get(b"ToUnicode").and_then(|v| v.as_reference()) {
            Ok(id) => id,
            Err(_) => continue,
        };
        if let Ok(stream) = doc.get_object(to_u_id).and_then(|o| o.as_stream())
            && let Ok(data) = stream.decompressed_content()
        {
            let cmap = parse_to_unicode(&data);
            if !cmap.is_empty() {
                table.insert(fname, cmap);
            }
        }
    }
    table
}

pub(crate) fn extract_page_text_runs(doc: &Document, page: u32) -> anyhow::Result<Vec<TextRun>> {
    let page_id = get_page_id(doc, page)?;
    let raw = doc.get_page_content(page_id)?;
    let content = Content::decode(&raw)?;
    let cmap_table = build_cmap_table(doc, page_id);
    let (mut cx, mut cy, mut lx, mut ly) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut font_size = 12.0f32;
    let mut font_name = String::new();
    let mut runs = Vec::new();

    for op in &content.operations {
        match op.operator.as_str() {
            "BT" => {
                cx = 0.0;
                cy = 0.0;
                lx = 0.0;
                ly = 0.0;
            }
            "Tm" if op.operands.len() >= 6 => {
                cx = op.operands[4].as_f32().unwrap_or(0.0);
                cy = op.operands[5].as_f32().unwrap_or(0.0);
                lx = cx;
                ly = cy;
            }
            "Td" | "TD" if op.operands.len() >= 2 => {
                lx += op.operands[0].as_f32().unwrap_or(0.0);
                ly += op.operands[1].as_f32().unwrap_or(0.0);
                cx = lx;
                cy = ly;
            }
            "Tf" if op.operands.len() >= 2 => {
                font_name = op.operands[0]
                    .as_name()
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_default();
                font_size = op.operands[1].as_f32().unwrap_or(font_size);
            }
            "Tj" => {
                if let Some(bytes) = op.operands.first().and_then(|v| v.as_str().ok()) {
                    let cmap = cmap_table.get(&font_name);
                    let txt = decode_bytes_with_cmap(bytes, cmap);
                    let txt = txt.trim().to_string();
                    if !txt.is_empty() {
                        runs.push(TextRun {
                            x: cx,
                            y: cy,
                            font_name: font_name.clone(),
                            font_size,
                            text: txt,
                        });
                    }
                }
            }
            "TJ" => {
                let cmap = cmap_table.get(&font_name);
                let mut txt = String::new();
                if let Some(arr) = op.operands.first().and_then(|v| v.as_array().ok()) {
                    for item in arr {
                        if let Ok(bs) = item.as_str() {
                            txt.push_str(&decode_bytes_with_cmap(bs, cmap));
                        }
                    }
                }
                let txt = txt.trim().to_string();
                if !txt.is_empty() {
                    runs.push(TextRun {
                        x: cx,
                        y: cy,
                        font_name: font_name.clone(),
                        font_size,
                        text: txt,
                    });
                }
            }
            _ => {}
        }
    }

    Ok(runs)
}

pub fn text_info(
    input: PathBuf,
    page: u32,
    all_pages: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run: Vec<u32> = if all_pages {
        let mut ps: Vec<u32> = doc.get_pages().keys().cloned().collect();
        ps.sort();
        ps
    } else {
        vec![page]
    };
    for cur_page in pages_to_run {
        println!("\n====== 第 {} 页 ======", cur_page);
        for run in extract_page_text_runs(&doc, cur_page)? {
            println!(
                "  ({:.1},{:.1}) /{} {}pt  \"{}\"",
                run.x, run.y, run.font_name, run.font_size, run.text
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn inspect(
    input: PathBuf,
    page: u32,
    pages: Option<String>,
    all_pages: bool,
    show_annots: bool,
    show_resources: bool,
    show_raw: bool,
    show_text: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;
    for cur_page in &pages_to_run {
        let page_id = get_page_id(&doc, *cur_page)?;
        let page_dict = doc.get_object(page_id)?.as_dict()?;
        let show_all = !show_annots && !show_resources && !show_raw && !show_text;

        println!("\n========================================================");
        println!("          PDF 页面深度审计报告 (第 {} 页)", cur_page);
        println!("========================================================");

        if show_all || show_raw {
            let raw = doc.get_page_content(page_id)?;
            println!("\n[1. 原始数据摘要]");
            println!("  - 内容流大小: {} 字节", raw.len());
            println!(
                "  - Contents 引用: {:?}",
                page_dict
                    .get(b"Contents")
                    .map(|c| format!("{:?}", c))
                    .unwrap_or("直接嵌入".into())
            );
        }

        if show_all || show_text {
            println!("\n[2. 文本与位置信息]");
            let raw = doc.get_page_content(page_id)?;
            let content = Content::decode(&raw)?;
            let cmap_table = build_cmap_table(&doc, page_id);
            let (mut cx, mut cy, mut lx, mut ly) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            let mut font_name = String::new();

            for op in &content.operations {
                match op.operator.as_str() {
                    "BT" => {
                        cx = 0.0;
                        cy = 0.0;
                        lx = 0.0;
                        ly = 0.0;
                    }
                    "Tm" if op.operands.len() >= 6 => {
                        cx = op.operands[4].as_f32().unwrap_or(0.0);
                        cy = op.operands[5].as_f32().unwrap_or(0.0);
                        lx = cx;
                        ly = cy;
                    }
                    "Td" | "TD" if op.operands.len() >= 2 => {
                        lx += op.operands[0].as_f32().unwrap_or(0.0);
                        ly += op.operands[1].as_f32().unwrap_or(0.0);
                        cx = lx;
                        cy = ly;
                    }
                    "Tf" if !op.operands.is_empty() => {
                        font_name = op.operands[0]
                            .as_name()
                            .map(|n| String::from_utf8_lossy(n).to_string())
                            .unwrap_or_default();
                    }
                    "Tj" => {
                        if let Some(bytes) = op.operands.first().and_then(|v| v.as_str().ok()) {
                            let cmap = cmap_table.get(&font_name);
                            let txt = decode_bytes_with_cmap(bytes, cmap);
                            if !txt.trim().is_empty() {
                                println!("  - \"{}\" ----> ({:.2}, {:.2})", txt, cx, cy);
                            }
                        }
                    }
                    "TJ" => {
                        let cmap = cmap_table.get(&font_name);
                        let mut txt = String::new();
                        if let Some(arr) = op.operands.first().and_then(|v| v.as_array().ok()) {
                            for item in arr {
                                if let Ok(bs) = item.as_str() {
                                    txt.push_str(&decode_bytes_with_cmap(bs, cmap));
                                }
                            }
                        }
                        if !txt.trim().is_empty() {
                            println!("  - \"{}\" ----> ({:.2}, {:.2})", txt, cx, cy);
                        }
                    }
                    _ => {}
                }
            }
        }

        if show_all || show_annots {
            println!("\n[3. 页面注释文本]");
            if let Ok(arr) = page_dict.get(b"Annots").and_then(|o| o.as_array()) {
                for (i, obj) in arr.iter().enumerate() {
                    if let Ok(id) = obj.as_reference()
                        && let Ok(d) = doc.get_object(id)?.as_dict()
                    {
                        let sub = d.get(b"Subtype").and_then(|s| s.as_name()).unwrap_or(b"?");
                        let decoded_ct = if let Ok(ct_obj) = d.get(b"Contents") {
                            lopdf::decode_text_string(ct_obj).unwrap_or_else(|_| {
                                String::from_utf8_lossy(ct_obj.as_str().unwrap_or(b"")).to_string()
                            })
                        } else {
                            String::new()
                        };
                        println!(
                            "  [#{} /{}] {}",
                            i + 1,
                            String::from_utf8_lossy(sub),
                            decoded_ct
                        );
                    }
                }
            } else {
                println!("  无注释。");
            }
        }

        if show_all || show_resources {
            println!("\n[4. 资源完整性检查]");
            if let Ok(res) = resolve_resources(&doc, page_id) {
                let raw = doc.get_page_content(page_id)?;
                let content = Content::decode(&raw)?;
                let mut used: std::collections::HashSet<String> = Default::default();
                for op in &content.operations {
                    if op.operator == "Tf"
                        && let Some(n) = op.operands.first().and_then(|o| o.as_name().ok())
                    {
                        used.insert(String::from_utf8_lossy(n).to_string());
                    }
                }
                let defined = res.get(b"Font").and_then(|f| f.as_dict()).ok();
                for font in &used {
                    match defined {
                        Some(d) if d.has(font.as_bytes()) => println!("  ✅ 字体 /{} 正常。", font),
                        Some(_) => println!("  ❌ 字体 /{} 在资源字典中未定义！", font),
                        None => println!("  ❌ 无字体资源，但内容流调用了 /{}。", font),
                    }
                }
            }
        }
        println!("========================================================");
    }
    Ok(())
}

fn get_page_id(doc: &Document, page: u32) -> anyhow::Result<lopdf::ObjectId> {
    doc.get_pages()
        .get(&page)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", page))
}
