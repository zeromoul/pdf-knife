//! # PDF 结构分析模块
//!
//! 输出树形结构的 PDF 文档分析报告，包括：
//! - 页面属性（MediaBox / CropBox / Rotate）
//! - 文字内容（字体、大小、坐标、内容）
//! - 图片资源（类型、尺寸、位置、颜色空间）
//! - 表单字段（类型、位置、值）
//! - 注释（类型、内容、作者）
//! - 图形状态（透明度等）
//! - 操作符统计
//! - 水印风险提示

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;
use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════
// 公开入口
// ═══════════════════════════════════════════════════════════════

pub fn analyze(
    input: PathBuf,
    page: u32,
    pages: Option<String>,
    all_pages: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;

    println!("📄 PDF 文档结构分析报告");
    println!("═══════════════════════════════════════════════\n");

    for cur_page in &pages_to_run {
        analyze_page(&doc, *cur_page)?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 数据结构
// ═══════════════════════════════════════════════════════════════

struct PageInfo {
    page_num: u32,
    obj_id: ObjectId,
    media_box: [f64; 4],
    crop_box: Option<[f64; 4]>,
    rotate: i64,
}

struct TextSegment {
    text: String,
    font_res: String,
    font_size: f64,
    x: f64,
    y: f64,
    opacity: f64,
    is_rotated: bool,
}

struct ImageUsage {
    res_name: String,
    obj_id: ObjectId,
    /// 页面坐标系中的位置 (左下角 x, y)
    pos_x: f64,
    pos_y: f64,
    /// 页面上的宽高（points）
    page_w: f64,
    page_h: f64,
    /// 像素尺寸
    img_w: i64,
    img_h: i64,
    filter: String,
    colorspace: String,
    size_bytes: usize,
    has_smask: bool,
    has_image_mask: bool,
}

struct AnnotInfo {
    idx: usize,
    obj_id: ObjectId,
    subtype: String,
    rect: [f64; 4],
    contents: String,
    author: String,
    uri: String,
}

struct FormFieldInfo {
    idx: usize,
    obj_id: ObjectId,
    name: String,
    field_type: String,
    rect: [f64; 4],
    value: String,
    action: String,
}

struct GsInfo {
    name: String,
    fill_opacity: f64,
    stroke_opacity: f64,
}

#[derive(Default)]
struct ContentStats {
    total: usize,
    text: usize,
    path: usize,
    color: usize,
    state: usize,
    xobject: usize,
    marked: usize,
}

// ═══════════════════════════════════════════════════════════════
// 页面分析主函数
// ═══════════════════════════════════════════════════════════════

fn analyze_page(doc: &Document, page_num: u32) -> anyhow::Result<()> {
    let page_id = doc
        .get_pages()
        .get(&page_num)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", page_num))?;

    let page_dict = doc.get_object(page_id)?.as_dict()?.clone();

    let page_info = collect_page_info(doc, page_num, page_id, &page_dict);
    let resources = crate::util::resolve_resources(doc, page_id).ok();

    // 构建 ToUnicode CMap 表（用于正确解码字体文字）
    let cmap_table = crate::cmd::text::build_cmap_table(doc, page_id);

    // 解析内容流
    let (text_segs, img_usages, stats) = if let Ok(raw) = doc.get_page_content(page_id) {
        parse_content(doc, &raw, &resources, &cmap_table)
    } else {
        (vec![], vec![], ContentStats::default())
    };

    // 收集 ExtGState 透明度
    let gs_list = collect_extgstate(doc, &resources);

    // 注释 & 表单
    let annots = collect_annotations(doc, &page_dict);
    let form_fields = collect_form_fields(doc, page_id);

    // ── 打印 ──
    println!(
        "第 {} 页 (对象 ID: {} 0 R)",
        page_info.page_num, page_info.obj_id.0
    );
    print_page_info(&page_info);
    print_text_section(&text_segs);
    print_image_section(&img_usages);
    print_form_section(&form_fields);
    print_annot_section(&annots);
    print_gs_section(&gs_list);
    print_stats_section(&stats);
    print_wm_warnings(&text_segs, &img_usages, &gs_list, &page_info);

    println!("═══════════════════════════════════════════════\n");
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 数据收集
// ═══════════════════════════════════════════════════════════════

fn collect_page_info(
    doc: &Document,
    page_num: u32,
    page_id: ObjectId,
    page_dict: &Dictionary,
) -> PageInfo {
    let media_box = read_box(doc, page_dict, b"MediaBox").unwrap_or([0.0, 0.0, 595.0, 842.0]);
    let crop_box = read_box(doc, page_dict, b"CropBox");
    let rotate = page_dict
        .get(b"Rotate")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    PageInfo {
        page_num,
        obj_id: page_id,
        media_box,
        crop_box,
        rotate,
    }
}

fn read_box(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<[f64; 4]> {
    let arr = dict.get(key).ok().and_then(|v| {
        if let Ok(id) = v.as_reference() {
            doc.get_object(id)
                .ok()
                .and_then(|o| o.as_array().ok().cloned())
        } else {
            v.as_array().ok().cloned()
        }
    })?;
    if arr.len() < 4 {
        return None;
    }
    Some([
        obj_to_f64(&arr[0]),
        obj_to_f64(&arr[1]),
        obj_to_f64(&arr[2]),
        obj_to_f64(&arr[3]),
    ])
}

fn obj_to_f64(o: &Object) -> f64 {
    match o {
        Object::Integer(n) => *n as f64,
        Object::Real(f) => *f as f64,
        _ => 0.0,
    }
}

// ── 内容流解析 ──────────────────────────────────────────────────

/// 当前图形状态（简化）
#[derive(Clone)]
struct GState {
    /// 当前变换矩阵 [a b c d e f]
    ctm: [f64; 6],
    fill_opacity: f64,
    stroke_opacity: f64,
    font_name: String,
    font_size: f64,
    /// 文字矩阵 [a b c d e f]
    tm: [f64; 6],
}

impl GState {
    fn identity() -> Self {
        GState {
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            font_name: String::new(),
            font_size: 12.0,
            tm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
    fn concat_ctm(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        let [sa, sb, sc, sd, se, sf] = self.ctm;
        self.ctm = [
            sa * a + sb * c,
            sa * b + sb * d,
            sc * a + sd * c,
            sc * b + sd * d,
            se * a + sf * c + e,
            se * b + sf * d + f,
        ];
    }
    fn page_pos(&self) -> (f64, f64) {
        // 文字位置 = Tm 的平移分量经 CTM 变换后的页面坐标
        let tx = self.tm[4];
        let ty = self.tm[5];
        let x = self.ctm[0] * tx + self.ctm[2] * ty + self.ctm[4];
        let y = self.ctm[1] * tx + self.ctm[3] * ty + self.ctm[5];
        (x, y)
    }
    fn image_pos(&self) -> (f64, f64, f64, f64) {
        // 图片单位矩形变换后的位置和尺寸
        let e = self.ctm[4];
        let f = self.ctm[5];
        let w = (self.ctm[0] * self.ctm[0] + self.ctm[1] * self.ctm[1]).sqrt();
        let h = (self.ctm[2] * self.ctm[2] + self.ctm[3] * self.ctm[3]).sqrt();
        (e, f, w, h)
    }
    fn is_rotated(&self) -> bool {
        self.tm[1].abs() > 0.01 || self.tm[2].abs() > 0.01
    }
    fn min_opacity(&self) -> f64 {
        self.fill_opacity.min(self.stroke_opacity)
    }
}

fn parse_content(
    doc: &Document,
    raw: &[u8],
    resources: &Option<lopdf::Dictionary>,
    cmap_table: &HashMap<String, HashMap<u16, String>>,
) -> (Vec<TextSegment>, Vec<ImageUsage>, ContentStats) {
    let ops = match lopdf::content::Content::decode(raw) {
        Ok(c) => c.operations,
        Err(_) => return (vec![], vec![], ContentStats::default()),
    };

    // 收集 ExtGState 透明度
    let gs_opacity: HashMap<String, (f64, f64)> = resources
        .as_ref()
        .and_then(|r| r.get(b"ExtGState").and_then(|v| v.as_dict()).ok())
        .map(|d| {
            d.iter()
                .filter_map(|(k, v)| {
                    let id = v.as_reference().ok()?;
                    let gs = doc.get_object(id).and_then(|o| o.as_dict()).ok()?;
                    let ca = gs.get(b"ca").and_then(|v| v.as_f32()).unwrap_or(1.0) as f64;
                    let ca_s = gs.get(b"CA").and_then(|v| v.as_f32()).unwrap_or(1.0) as f64;
                    Some((String::from_utf8_lossy(k).to_string(), (ca, ca_s)))
                })
                .collect()
        })
        .unwrap_or_default();

    // 收集 XObject 信息
    let xobj_map: HashMap<String, ObjectId> = resources
        .as_ref()
        .and_then(|r| r.get(b"XObject").and_then(|v| v.as_dict()).ok())
        .map(|d| {
            d.iter()
                .filter_map(|(k, v)| {
                    let id = v.as_reference().ok()?;
                    Some((String::from_utf8_lossy(k).to_string(), id))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut state_stack: Vec<GState> = Vec::new();
    let mut gs = GState::identity();
    let mut in_bt = false;
    let mut cur_text = String::new();
    let mut text_segs: Vec<TextSegment> = Vec::new();
    let mut img_usages: Vec<ImageUsage> = Vec::new();
    let mut stats = ContentStats::default();

    for op in &ops {
        stats.total += 1;
        match op.operator.as_str() {
            // ── 图形状态 ──
            "q" => state_stack.push(gs.clone()),
            "Q" => {
                if let Some(s) = state_stack.pop() {
                    gs = s;
                }
            }
            "cm" if op.operands.len() >= 6 => {
                let v: Vec<f64> = op.operands.iter().map(obj_to_f64).collect();
                gs.concat_ctm(v[0], v[1], v[2], v[3], v[4], v[5]);
                stats.state += 1;
            }
            "gs" => {
                stats.state += 1;
                if let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) {
                    let key = String::from_utf8_lossy(name).to_string();
                    if let Some(&(ca, ca_s)) = gs_opacity.get(&key) {
                        gs.fill_opacity = ca;
                        gs.stroke_opacity = ca_s;
                    }
                }
            }
            "w" | "J" | "j" | "M" | "d" | "ri" | "i" => stats.state += 1,
            "CS" | "cs" | "SC" | "SCN" | "sc" | "scn" | "G" | "g" | "RG" | "rg" | "K" | "k" => {
                stats.color += 1
            }

            // ── 文字 ──
            "BT" => {
                in_bt = true;
                cur_text.clear();
                gs.tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                stats.text += 1;
            }
            "ET" => {
                if in_bt && !cur_text.trim().is_empty() {
                    let (x, y) = gs.page_pos();
                    text_segs.push(TextSegment {
                        text: cur_text.clone(),
                        font_res: gs.font_name.clone(),
                        font_size: gs.font_size,
                        x,
                        y,
                        opacity: gs.min_opacity(),
                        is_rotated: gs.is_rotated(),
                    });
                }
                in_bt = false;
                cur_text.clear();
                stats.text += 1;
            }
            "Tf" if op.operands.len() >= 2 => {
                gs.font_name = op.operands[0]
                    .as_name()
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_default();
                gs.font_size = obj_to_f64(&op.operands[1]);
                stats.text += 1;
            }
            "Td" | "TD" if op.operands.len() >= 2 => {
                let tx = obj_to_f64(&op.operands[0]);
                let ty = obj_to_f64(&op.operands[1]);
                // Td: Tm = [[1 0 0 1 tx ty]] × Tlm
                let [_a, _b, _c, _d, te, tf] = gs.tm;
                gs.tm = [1.0, 0.0, 0.0, 1.0, te + tx, tf + ty];
                stats.text += 1;
            }
            "Tm" if op.operands.len() >= 6 => {
                let v: Vec<f64> = op.operands.iter().map(obj_to_f64).collect();
                gs.tm = [v[0], v[1], v[2], v[3], v[4], v[5]];
                stats.text += 1;
            }
            "T*" => {
                let [_a, _b, _c, _d, te, tf] = gs.tm;
                gs.tm = [1.0, 0.0, 0.0, 1.0, te, tf - gs.font_size];
                stats.text += 1;
            }
            "Tj" if in_bt => {
                if let Some(s) = decode_text_op(&op.operands, cmap_table.get(&gs.font_name)) {
                    if !cur_text.is_empty() {
                        cur_text.push(' ');
                    }
                    cur_text.push_str(&s);
                }
                stats.text += 1;
            }
            "TJ" if in_bt => {
                if let Some(s) = decode_tj_op(&op.operands, cmap_table.get(&gs.font_name)) {
                    if !cur_text.is_empty() {
                        cur_text.push(' ');
                    }
                    cur_text.push_str(&s);
                }
                stats.text += 1;
            }
            "'" | "\"" if in_bt => {
                if let Some(s) = decode_text_op(&op.operands, cmap_table.get(&gs.font_name)) {
                    if !cur_text.is_empty() {
                        cur_text.push('\n');
                    }
                    cur_text.push_str(&s);
                }
                stats.text += 1;
            }
            "Tc" | "Tw" | "Tz" | "TL" | "Tr" | "Ts" => stats.text += 1,

            // ── 路径 ──
            "m" | "l" | "c" | "v" | "y" | "h" | "re" | "S" | "s" | "f" | "F" | "f*" | "B"
            | "B*" | "b" | "b*" | "n" | "W" | "W*" => stats.path += 1,

            // ── XObject ──
            "Do" => {
                stats.xobject += 1;
                if let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) {
                    let res_name = String::from_utf8_lossy(name).to_string();
                    if let Some(&xobj_id) = xobj_map.get(&res_name)
                        && let Ok(stream) = doc.get_object(xobj_id).and_then(|o| o.as_stream())
                    {
                        let subtype = stream
                            .dict
                            .get(b"Subtype")
                            .and_then(|v| v.as_name())
                            .unwrap_or(b"");
                        if subtype == b"Image" {
                            let (px, py, pw, ph) = gs.image_pos();
                            let img_w = stream
                                .dict
                                .get(b"Width")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let img_h = stream
                                .dict
                                .get(b"Height")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let filter = stream
                                .dict
                                .get(b"Filter")
                                .and_then(|v| v.as_name())
                                .map(filter_to_label)
                                .unwrap_or_else(|_| "Raw".into());
                            let cs = stream
                                .dict
                                .get(b"ColorSpace")
                                .map(cs_to_label)
                                .unwrap_or_else(|_| "?".into());
                            let has_smask = stream.dict.get(b"SMask").is_ok();
                            let has_image_mask = stream
                                .dict
                                .get(b"ImageMask")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            img_usages.push(ImageUsage {
                                res_name,
                                obj_id: xobj_id,
                                pos_x: px,
                                pos_y: py,
                                page_w: pw,
                                page_h: ph,
                                img_w,
                                img_h,
                                filter,
                                colorspace: cs,
                                size_bytes: stream.content.len(),
                                has_smask,
                                has_image_mask,
                            });
                        }
                    }
                }
            }

            // ── 标记内容 ──
            "BMC" | "BDC" | "EMC" | "MP" | "DP" => stats.marked += 1,

            _ => {}
        }
    }

    (text_segs, img_usages, stats)
}

fn decode_text_op(operands: &[Object], cmap: Option<&HashMap<u16, String>>) -> Option<String> {
    let o = operands.first()?;
    if let Ok(bytes) = o.as_str() {
        let s = crate::cmd::text::decode_bytes_with_cmap(bytes, cmap);
        if !s.is_empty() {
            return Some(s);
        }
    }
    // 兜底：尝试 lopdf 内置的 UTF-16BE 解码
    lopdf::decode_text_string(o).ok()
}

fn decode_tj_op(operands: &[Object], cmap: Option<&HashMap<u16, String>>) -> Option<String> {
    let arr = operands.first()?.as_array().ok()?;
    let mut s = String::new();
    for item in arr {
        if let Ok(bytes) = item.as_str() {
            s.push_str(&crate::cmd::text::decode_bytes_with_cmap(bytes, cmap));
        }
        // 数字元素为字距调整值，直接跳过
    }
    if s.is_empty() { None } else { Some(s) }
}

fn filter_to_label(name: &[u8]) -> String {
    match name {
        b"DCTDecode" => "JPEG".into(),
        b"FlateDecode" => "PNG/Deflate".into(),
        b"CCITTFaxDecode" => "TIFF/Fax".into(),
        b"JBIG2Decode" => "JBIG2".into(),
        b"JPXDecode" => "JPEG2000".into(),
        b"LZWDecode" => "LZW".into(),
        b"RunLengthDecode" => "RLE".into(),
        other => String::from_utf8_lossy(other).to_string(),
    }
}

fn cs_to_label(v: &Object) -> String {
    match v {
        Object::Name(n) => match n.as_slice() {
            b"DeviceRGB" => "RGB".into(),
            b"DeviceCMYK" => "CMYK".into(),
            b"DeviceGray" => "Gray".into(),
            other => String::from_utf8_lossy(other).to_string(),
        },
        Object::Array(arr) => arr
            .first()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_else(|| "?".into()),
        _ => "?".into(),
    }
}

// ── ExtGState 收集 ──────────────────────────────────────────────

fn collect_extgstate(doc: &Document, resources: &Option<Dictionary>) -> Vec<GsInfo> {
    let mut list = Vec::new();
    let ext_gs = match resources
        .as_ref()
        .and_then(|r| r.get(b"ExtGState").and_then(|v| v.as_dict()).ok())
    {
        Some(d) => d.clone(),
        None => return list,
    };
    for (name, val) in &ext_gs {
        let gs_dict = if let Ok(id) = val.as_reference() {
            match doc.get_object(id).and_then(|o| o.as_dict()) {
                Ok(d) => d.clone(),
                Err(_) => continue,
            }
        } else {
            match val.as_dict() {
                Ok(d) => d.clone(),
                Err(_) => continue,
            }
        };
        let ca = gs_dict.get(b"ca").and_then(|v| v.as_f32()).unwrap_or(1.0) as f64;
        let ca_s = gs_dict.get(b"CA").and_then(|v| v.as_f32()).unwrap_or(1.0) as f64;
        list.push(GsInfo {
            name: String::from_utf8_lossy(name).to_string(),
            fill_opacity: ca,
            stroke_opacity: ca_s,
        });
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

// ── 注释收集 ────────────────────────────────────────────────────

fn collect_annotations(doc: &Document, page_dict: &Dictionary) -> Vec<AnnotInfo> {
    let mut list = Vec::new();
    let annots_obj = match page_dict.get(b"Annots") {
        Ok(o) => o.clone(),
        Err(_) => return list,
    };
    let arr = if let Ok(id) = annots_obj.as_reference() {
        match doc.get_object(id).and_then(|o| o.as_array()) {
            Ok(a) => a.clone(),
            Err(_) => return list,
        }
    } else {
        match annots_obj.as_array() {
            Ok(a) => a.clone(),
            Err(_) => return list,
        }
    };
    for (idx, item) in arr.iter().enumerate() {
        let (ann_id, dict) = if let Ok(id) = item.as_reference() {
            match doc.get_object(id).and_then(|o| o.as_dict()) {
                Ok(d) => (id, d.clone()),
                Err(_) => continue,
            }
        } else if let Ok(d) = item.as_dict() {
            ((0, 0), d.clone())
        } else {
            continue;
        };
        let subtype = dict
            .get(b"Subtype")
            .and_then(|v| v.as_name())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_default();
        let rect = read_box_from_obj(&dict.get(b"Rect").cloned().unwrap_or(Object::Null))
            .unwrap_or([0.0; 4]);
        let contents = dict
            .get(b"Contents")
            .ok()
            .and_then(|v| lopdf::decode_text_string(v).ok())
            .unwrap_or_default();
        let author = dict
            .get(b"T")
            .ok()
            .and_then(|v| lopdf::decode_text_string(v).ok())
            .unwrap_or_default();
        let uri = dict
            .get(b"A")
            .and_then(|v| v.as_dict())
            .ok()
            .and_then(|a| a.get(b"URI").ok())
            .and_then(|u| lopdf::decode_text_string(u).ok())
            .unwrap_or_default();
        list.push(AnnotInfo {
            idx: idx + 1,
            obj_id: ann_id,
            subtype,
            rect,
            contents,
            author,
            uri,
        });
    }
    list
}

fn read_box_from_obj(obj: &Object) -> Option<[f64; 4]> {
    let arr = obj.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    Some([
        obj_to_f64(&arr[0]),
        obj_to_f64(&arr[1]),
        obj_to_f64(&arr[2]),
        obj_to_f64(&arr[3]),
    ])
}

// ── 表单字段收集 ────────────────────────────────────────────────

fn collect_form_fields(doc: &Document, page_id: ObjectId) -> Vec<FormFieldInfo> {
    let mut list = Vec::new();
    let root_id = match doc.trailer.get(b"Root").and_then(|o| o.as_reference()) {
        Ok(id) => id,
        Err(_) => return list,
    };
    let acro_dict: Dictionary = {
        let root = match doc.get_object(root_id).and_then(|o| o.as_dict()) {
            Ok(d) => d.clone(),
            Err(_) => return list,
        };
        let acro = match root.get(b"AcroForm") {
            Ok(o) => o.clone(),
            Err(_) => return list,
        };
        if let Ok(id) = acro.as_reference() {
            match doc.get_object(id).and_then(|o| o.as_dict()) {
                Ok(d) => d.clone(),
                Err(_) => return list,
            }
        } else {
            match acro.as_dict() {
                Ok(d) => d.clone(),
                Err(_) => return list,
            }
        }
    };

    // 构建 page_id → [field_id] 映射
    let fields_obj = match acro_dict.get(b"Fields") {
        Ok(o) => o.clone(),
        Err(_) => return list,
    };
    let fields_arr = match fields_obj {
        Object::Array(a) => a,
        Object::Reference(r) => match doc.get_object(r).and_then(|o| o.as_array()) {
            Ok(a) => a.clone(),
            Err(_) => return list,
        },
        _ => return list,
    };

    let mut all_field_ids: Vec<ObjectId> = Vec::new();
    for item in &fields_arr {
        if let Ok(id) = item.as_reference() {
            all_field_ids.push(id);
            if let Ok(d) = doc.get_object(id).and_then(|o| o.as_dict())
                && let Ok(kids) = d.get(b"Kids").and_then(|v| v.as_array())
            {
                for k in kids {
                    if let Ok(kid_id) = k.as_reference() {
                        all_field_ids.push(kid_id);
                    }
                }
            }
        }
    }

    for (idx, fid) in all_field_ids.iter().enumerate() {
        let dict = match doc.get_object(*fid).and_then(|o| o.as_dict()) {
            Ok(d) => d.clone(),
            Err(_) => continue,
        };
        // 只收集属于本页的字段（P 引用本页，或 Rect 存在）
        let on_this_page = dict
            .get(b"P")
            .and_then(|v| v.as_reference())
            .map(|id| id == page_id)
            .unwrap_or(false)
            || dict.get(b"Rect").is_ok();
        if !on_this_page {
            continue;
        }
        let name = dict
            .get(b"T")
            .ok()
            .and_then(|v| lopdf::decode_text_string(v).ok())
            .unwrap_or_default();
        let field_type = dict
            .get(b"FT")
            .and_then(|v| v.as_name())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_else(|_| "?".into());
        let rect = dict
            .get(b"Rect")
            .ok()
            .and_then(read_box_from_obj)
            .unwrap_or([0.0; 4]);
        let value = dict
            .get(b"V")
            .ok()
            .and_then(|v| lopdf::decode_text_string(v).ok())
            .unwrap_or_default();
        let action = dict
            .get(b"A")
            .and_then(|v| v.as_dict())
            .ok()
            .and_then(|a| {
                a.get(b"S")
                    .and_then(|s| s.as_name())
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .ok()
            })
            .unwrap_or_default();
        list.push(FormFieldInfo {
            idx: idx + 1,
            obj_id: *fid,
            name,
            field_type,
            rect,
            value,
            action,
        });
    }
    list
}

// ═══════════════════════════════════════════════════════════════
// 打印函数
// ═══════════════════════════════════════════════════════════════

/// 检测页面尺寸名称
fn page_size_name(w: f64, h: f64) -> &'static str {
    let (w, h) = if w < h { (w, h) } else { (h, w) };
    match (w.round() as i64, h.round() as i64) {
        (595, 842) => "A4",
        (420, 595) => "A5",
        (842, 1190) => "A3",
        (612, 792) => "Letter",
        (612, 1008) => "Legal",
        _ => "",
    }
}

fn print_page_info(info: &PageInfo) {
    println!("├─ 📦 页面属性");
    let [x0, y0, x1, y1] = info.media_box;
    let w = x1 - x0;
    let h = y1 - y0;
    let size_hint = page_size_name(w, h);
    let size_str = if size_hint.is_empty() {
        format!("[{} {} {} {}]", x0, y0, x1, y1)
    } else {
        format!("[{} {} {} {}] ({})", x0, y0, x1, y1, size_hint)
    };
    println!("│  ├─ MediaBox: {}", size_str);
    if let Some([cx0, cy0, cx1, cy1]) = info.crop_box {
        println!("│  ├─ CropBox:  [{} {} {} {}]", cx0, cy0, cx1, cy1);
    }
    println!("│  └─ Rotate:   {}°", info.rotate);
    println!("│");
}

/// 安全截断字符串到最多 max_chars 个字符
fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", collected)
    } else {
        collected
    }
}

fn print_text_section(segs: &[TextSegment]) {
    if segs.is_empty() {
        return;
    }
    println!("├─ 📝 文字内容 (共 {} 段)", segs.len());
    for (i, seg) in segs.iter().enumerate() {
        let is_last = i == segs.len() - 1;
        let branch = if is_last { "└─" } else { "├─" };
        let cont = if is_last { " " } else { "│" };
        // 截断过长的文字
        let text = truncate_str(&seg.text, 37);
        // 过滤掉乱码（非打印字符超过一半则跳过显示）
        let printable: usize = text.chars().filter(|c| !c.is_control()).count();
        let display_text = if printable * 2 < text.chars().count() {
            format!("<编码文本 {} 字节>", seg.text.len())
        } else {
            format!("\"{}\"", text)
        };
        println!("│  {} 段 {}: {}", branch, i + 1, display_text);
        println!("│  {}    ├─ 字体: /{}", cont, seg.font_res);
        println!("│  {}    ├─ 大小: {}pt", cont, seg.font_size);
        let (x, y) = (seg.x.round() as i64, seg.y.round() as i64);
        print!("│  {}    ├─ 位置: ({}, {})", cont, x, y);
        if seg.is_rotated {
            print!("  [旋转]");
        }
        println!();
        println!(
            "│  {}    └─ 透明度: {:.0}%{}",
            cont,
            seg.opacity * 100.0,
            if seg.opacity < 0.9 {
                "  ⚠️ 半透明"
            } else {
                ""
            }
        );
    }
    println!("│");
}

fn print_image_section(imgs: &[ImageUsage]) {
    if imgs.is_empty() {
        return;
    }
    println!("├─ 🖼️  图片资源 (共 {} 个)", imgs.len());
    for (i, img) in imgs.iter().enumerate() {
        let is_last = i == imgs.len() - 1;
        let branch = if is_last { "└─" } else { "├─" };
        let cont = if is_last { " " } else { "│" };
        println!("│  {} /{} (ID: {} 0 R)", branch, img.res_name, img.obj_id.0);
        println!("│  {}    ├─ 类型: {}", cont, img.filter);
        println!("│  {}    ├─ 像素: {}×{}", cont, img.img_w, img.img_h);
        let kb = img.size_bytes as f64 / 1024.0;
        println!("│  {}    ├─ 大小: {:.1} KB", cont, kb);
        println!(
            "│  {}    ├─ 位置: ({:.0}, {:.0})  页面尺寸: {:.0}×{:.0}pt",
            cont, img.pos_x, img.pos_y, img.page_w, img.page_h
        );
        let cs = &img.colorspace;
        let mut flags = Vec::new();
        if img.has_smask {
            flags.push("含SMask透明通道");
        }
        if img.has_image_mask {
            flags.push("遮罩图");
        }
        let flag_str = if flags.is_empty() {
            String::new()
        } else {
            format!("  ⚠️ {}", flags.join(", "))
        };
        println!("│  {}    └─ 颜色空间: {}{}", cont, cs, flag_str);
    }
    println!("│");
}

fn print_form_section(fields: &[FormFieldInfo]) {
    if fields.is_empty() {
        return;
    }
    println!("├─ 📋 表单字段 (共 {} 个)", fields.len());
    for (i, f) in fields.iter().enumerate() {
        let is_last = i == fields.len() - 1;
        let branch = if is_last { "└─" } else { "├─" };
        let cont = if is_last { " " } else { "│" };
        let label = if f.name.is_empty() {
            format!("字段 #{}", f.idx)
        } else {
            format!("\"{}\"", f.name)
        };
        println!("│  {} {} (ID: {} 0 R)", branch, label, f.obj_id.0);
        println!("│  {}    ├─ 类型: /{}", cont, f.field_type);
        let [rx0, ry0, rx1, ry1] = f.rect;
        println!(
            "│  {}    ├─ 位置: [{:.0} {:.0} {:.0} {:.0}]",
            cont, rx0, ry0, rx1, ry1
        );
        if !f.value.is_empty() {
            println!("│  {}    ├─ 当前值: \"{}\"", cont, f.value);
        }
        if !f.action.is_empty() {
            println!("│  {}    └─ 动作: /{}", cont, f.action);
        } else {
            println!("│  {}    └─ 动作: 无", cont);
        }
    }
    println!("│");
}

fn print_annot_section(annots: &[AnnotInfo]) {
    if annots.is_empty() {
        return;
    }
    println!("├─ 💬 注释 (共 {} 个)", annots.len());
    for (i, ann) in annots.iter().enumerate() {
        let is_last = i == annots.len() - 1;
        let branch = if is_last { "└─" } else { "├─" };
        let cont = if is_last { " " } else { "│" };
        println!(
            "│  {} #{} /{} (ID: {} 0 R)",
            branch, ann.idx, ann.subtype, ann.obj_id.0
        );
        let [rx0, ry0, rx1, ry1] = ann.rect;
        println!(
            "│  {}    ├─ 位置: [{:.0} {:.0} {:.0} {:.0}]",
            cont, rx0, ry0, rx1, ry1
        );
        if !ann.contents.is_empty() {
            let c = truncate_str(&ann.contents, 37);
            println!("│  {}    ├─ 内容: \"{}\"", cont, c);
        }
        if !ann.author.is_empty() {
            println!("│  {}    ├─ 作者: {}", cont, ann.author);
        }
        if !ann.uri.is_empty() {
            println!("│  {}    └─ URI: {}", cont, ann.uri);
        } else {
            println!("│  {}    └─ (无 URI)", cont);
        }
    }
    println!("│");
}

fn print_gs_section(gs_list: &[GsInfo]) {
    let notable: Vec<&GsInfo> = gs_list
        .iter()
        .filter(|g| g.fill_opacity < 1.0 || g.stroke_opacity < 1.0)
        .collect();
    if notable.is_empty() {
        return;
    }
    println!("├─ 🔧 图形状态 (含透明度设置 {} 个)", notable.len());
    for (i, gs) in notable.iter().enumerate() {
        let is_last = i == notable.len() - 1;
        let branch = if is_last { "└─" } else { "├─" };
        let wm_hint = if gs.fill_opacity < 0.8 || gs.stroke_opacity < 0.8 {
            "  ⚠️ 疑似水印透明度"
        } else {
            ""
        };
        println!(
            "│  {} /{}: fill={:.0}%  stroke={:.0}%{}",
            branch,
            gs.name,
            gs.fill_opacity * 100.0,
            gs.stroke_opacity * 100.0,
            wm_hint
        );
    }
    println!("│");
}

fn print_stats_section(stats: &ContentStats) {
    println!("└─ 📊 统计信息");
    println!("   ├─ 总操作符数: {}", stats.total);
    println!("   ├─ 文字操作:   {}", stats.text);
    println!("   ├─ 路径操作:   {}", stats.path);
    println!("   ├─ 颜色操作:   {}", stats.color);
    println!("   ├─ 状态变更:   {}", stats.state);
    println!("   ├─ XObject:    {}", stats.xobject);
    println!("   └─ 标记内容:   {}", stats.marked);
}

fn print_wm_warnings(
    text_segs: &[TextSegment],
    imgs: &[ImageUsage],
    gs_list: &[GsInfo],
    page_info: &PageInfo,
) {
    let mut warnings: Vec<String> = Vec::new();

    // 检查低透明度图形状态
    for gs in gs_list {
        if gs.fill_opacity < 0.8 || gs.stroke_opacity < 0.8 {
            warnings.push(format!(
                "图形状态 /{} 透明度 {:.0}%，疑似水印层",
                gs.name,
                gs.fill_opacity.min(gs.stroke_opacity) * 100.0
            ));
        }
    }

    // 检查半透明或旋转文字
    for seg in text_segs {
        if seg.opacity < 0.9 {
            let text_preview = truncate_str(&seg.text, 17);
            warnings.push(format!(
                "文字 \"{}\" 透明度 {:.0}%{}",
                text_preview,
                seg.opacity * 100.0,
                if seg.is_rotated { "，带旋转" } else { "" }
            ));
        } else if seg.is_rotated {
            let text_preview = truncate_str(&seg.text, 17);
            warnings.push(format!(
                "文字 \"{}\" 带旋转矩阵，疑似文字水印",
                text_preview
            ));
        }
    }

    // 检查覆盖整页的图片（宽高接近页面尺寸）
    let pw = page_info.media_box[2] - page_info.media_box[0];
    let ph = page_info.media_box[3] - page_info.media_box[1];
    for img in imgs {
        if img.has_smask || img.has_image_mask {
            warnings.push(format!(
                "图片 /{} 含透明通道 (SMask/ImageMask)，疑似图片水印",
                img.res_name
            ));
        } else if pw > 0.0 && ph > 0.0 {
            let cover_w = img.page_w / pw;
            let cover_h = img.page_h / ph;
            if cover_w > 0.5 && cover_h > 0.5 {
                warnings.push(format!(
                    "图片 /{} 覆盖页面 {:.0}%×{:.0}%，疑似全页水印背景",
                    img.res_name,
                    cover_w * 100.0,
                    cover_h * 100.0
                ));
            }
        }
    }

    if warnings.is_empty() {
        return;
    }
    println!();
    println!("⚠️  检测到可能的水印:");
    for w in &warnings {
        println!("   - {}", w);
    }
}
