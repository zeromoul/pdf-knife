//! # 水印去除模块

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════
// 公开入口
// ═══════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
pub fn remove_watermark(
    input: PathBuf,
    output: PathBuf,
    pages: Option<String>,
    all_pages: bool,
    page: u32,
    // 类型开关（全部 false → 等同全部启用）
    annot: bool,
    text: bool,
    curve: bool,
    path_wm: bool,
    trace: bool,
    form: bool,
    image: bool,
    pattern: bool,
    // 调整参数
    opacity_threshold: f32,
    keyword: Vec<String>,
    dry_run: bool,
    // 内容流搜索
    stream_search: Option<String>,
    search_resource_streams: bool,
    // 资源序号删除
    res_del: Option<String>,
    res_skip: Option<String>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let all_types = !annot
        && !text
        && !curve
        && !path_wm
        && !trace
        && !form
        && !image
        && !pattern
        && stream_search.is_none()
        && res_del.is_none()
        && res_skip.is_none();
    let do_annot = all_types || annot;
    let do_text = all_types || text;
    let do_curve = all_types || curve;
    let do_path = all_types || path_wm;
    let do_trace = all_types || trace;
    let do_form = all_types || form;
    let do_image = all_types || image;
    let do_pattern = all_types || pattern;

    let mut doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;
    let mut report = Report::new(dry_run);

    if do_annot {
        remove_annot_watermarks(&mut doc, &pages_to_run, &keyword, &mut report)?;
    }

    if do_form {
        remove_form_watermarks(&mut doc, &keyword, &mut report)?;
    }

    let content_needed = do_text || do_curve || do_path || do_trace || do_image || do_pattern;
    if content_needed {
        remove_content_watermarks(
            &mut doc,
            &pages_to_run,
            do_text,
            do_curve,
            do_path,
            do_trace,
            do_image,
            do_pattern,
            opacity_threshold,
            &keyword,
            &mut report,
        )?;
    }

    if let Some(ref pattern_str) = stream_search {
        remove_stream_search_watermarks(
            &mut doc,
            &pages_to_run,
            search_resource_streams,
            pattern_str,
            &mut report,
        )?;
    }

    if res_del.is_some() || res_skip.is_some() {
        remove_res_index_watermarks(
            &mut doc,
            &pages_to_run,
            res_del.as_deref(),
            res_skip.as_deref(),
            &mut report,
        )?;
    }

    if dry_run {
        println!("🔍 [dry-run] 以下内容将被去除（实际未写入文件）：");
        report.print();
        println!(
            "\n共检测到 {} 项水印。（dry-run 模式，未写入文件）",
            report.total()
        );
    } else {
        report.print();
        crate::util::save_document(&mut doc, &output)?;
        println!(
            "\n✅ 水印去除完成，共处理 {} 项。输出: {:?}",
            report.total(),
            output
        );
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 报告收集器
// ═══════════════════════════════════════════════════════════════

struct Report {
    dry_run: bool,
    entries: Vec<String>,
}

impl Report {
    fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            entries: Vec::new(),
        }
    }
    fn add(&mut self, msg: impl Into<String>) {
        self.entries.push(msg.into());
    }
    fn total(&self) -> usize {
        self.entries.len()
    }
    fn print(&self) {
        if self.entries.is_empty() {
            println!("  （未检测到任何水印）");
        } else {
            for e in &self.entries {
                let prefix = if self.dry_run {
                    "  [预览]"
                } else {
                    "  [已删]"
                };
                println!("{} {}", prefix, e);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 辅助：解析 BDC 操作符的内联属性字典
// ═══════════════════════════════════════════════════════════════

/// 从 BDC 操作符的操作数中提取 Subtype 名称。
/// BDC 操作数格式：[/TagName, <<dict>>] 或 [/TagName, ref]
/// 返回 (tag_name, subtype) 例如 ("Artifact", "Watermark")
fn parse_bdc_subtype(op: &lopdf::content::Operation) -> Option<(String, String)> {
    if op.operator != "BDC" && op.operator != "BMC" {
        return None;
    }
    let tag = op
        .operands
        .first()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).to_string())?;

    // 第二个操作数可能是内联字典
    if let Some(dict_obj) = op.operands.get(1)
        && let Ok(dict) = dict_obj.as_dict()
    {
        let subtype = dict
            .get(b"Subtype")
            .ok()
            .and_then(|v| v.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_default();
        return Some((tag, subtype));
    }
    Some((tag, String::new()))
}

// ═══════════════════════════════════════════════════════════════
// ① 注释水印
// ═══════════════════════════════════════════════════════════════

fn remove_annot_watermarks(
    doc: &mut Document,
    pages: &[u32],
    keywords: &[String],
    report: &mut Report,
) -> anyhow::Result<()> {
    let page_map = doc.get_pages();
    let page_ids: Vec<(u32, ObjectId)> = pages
        .iter()
        .filter_map(|p| page_map.get(p).map(|id| (*p, *id)))
        .collect();

    for (pg_num, page_id) in page_ids {
        let annots_arr: Vec<Object> = {
            let page_dict = match doc.get_object(page_id).and_then(|o| o.as_dict()) {
                Ok(d) => d.clone(),
                Err(_) => continue,
            };
            let annots_obj = match page_dict.get(b"Annots") {
                Ok(o) => o.clone(),
                Err(_) => continue,
            };
            let arr = if let Ok(id) = annots_obj.as_reference() {
                match doc.get_object(id).and_then(|o| o.as_array()) {
                    Ok(a) => a.clone(),
                    Err(_) => continue,
                }
            } else {
                match annots_obj.as_array() {
                    Ok(a) => a.clone(),
                    Err(_) => continue,
                }
            };
            arr.clone()
        };

        // 分两类：间接引用注释 和 内联字典注释
        let mut to_remove_ids: Vec<ObjectId> = Vec::new();
        // 内联水印注释：直接从 Annots 数组里删掉（按索引记录）
        let mut inline_wm_indices: Vec<usize> = Vec::new();

        for (idx, ann_obj) in annots_arr.iter().enumerate() {
            if let Ok(ann_id) = ann_obj.as_reference() {
                // 间接引用
                if is_watermark_annot(doc, ann_id, keywords) {
                    to_remove_ids.push(ann_id);
                }
            } else if let Ok(dict) = ann_obj.as_dict() {
                // 内联字典：直接判断
                if is_watermark_annot_dict(dict, keywords) {
                    inline_wm_indices.push(idx);
                }
            }
        }

        // 删除间接引用注释对象
        for id in &to_remove_ids {
            remove_annot_ref_from_page(doc, page_id, *id);
            delete_annot_object(doc, *id);
            report.add(format!(
                "[注释水印] 第{}页 注释 {} {} R",
                pg_num, id.0, id.1
            ));
        }

        // 删除内联注释：从 Annots 数组中按索引移除（倒序以免偏移）
        if !inline_wm_indices.is_empty() {
            // 解析 Annots 的存储位置（页面字典直接 or 间接对象）
            let page_dict = match doc.get_object(page_id).and_then(|o| o.as_dict()) {
                Ok(d) => d.clone(),
                Err(_) => {
                    report.add(format!(
                        "[注释水印] 第{}页 {} 个内联注释（跳过，页面字典不可变）",
                        pg_num,
                        inline_wm_indices.len()
                    ));
                    continue;
                }
            };
            let annots_is_inline = page_dict
                .get(b"Annots")
                .map(|o| o.as_array().is_ok())
                .unwrap_or(false);

            if annots_is_inline {
                // Annots 直接内联在页面字典里
                if let Ok(page_dict_mut) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut())
                    && let Ok(arr) = page_dict_mut
                        .get_mut(b"Annots")
                        .and_then(|o| o.as_array_mut())
                {
                    for &idx in inline_wm_indices.iter().rev() {
                        if idx < arr.len() {
                            arr.remove(idx);
                        }
                    }
                }
            } else if let Ok(annots_ref_id) =
                page_dict.get(b"Annots").and_then(|o| o.as_reference())
            {
                // Annots 是间接引用数组对象
                if let Ok(arr_obj) = doc
                    .get_object_mut(annots_ref_id)
                    .and_then(|o| o.as_array_mut())
                {
                    for &idx in inline_wm_indices.iter().rev() {
                        if idx < arr_obj.len() {
                            arr_obj.remove(idx);
                        }
                    }
                }
            }
            report.add(format!(
                "[注释水印] 第{}页 {} 个内联水印注释",
                pg_num,
                inline_wm_indices.len()
            ));
        }
    }
    Ok(())
}

fn is_watermark_annot(doc: &Document, ann_id: ObjectId, keywords: &[String]) -> bool {
    let dict = match doc.get_object(ann_id).and_then(|o| o.as_dict()) {
        Ok(d) => d,
        Err(_) => return false,
    };
    is_watermark_annot_dict(dict, keywords)
}

/// 对一个注释字典（无论间接引用还是内联）进行水印判断
fn is_watermark_annot_dict(dict: &lopdf::Dictionary, keywords: &[String]) -> bool {
    let subtype = dict
        .get(b"Subtype")
        .and_then(|v| v.as_name())
        .unwrap_or(b"");

    // 硬判断：Watermark / Stamp
    if subtype == b"Watermark" || subtype == b"Stamp" {
        return true;
    }
    // 印章类注释：GoldGrid:AddSeal / AddSeal / Seal 等第三方电子签章
    {
        let sub_str = String::from_utf8_lossy(subtype).to_lowercase();
        if sub_str.contains("seal") || sub_str.contains("stamp") || sub_str.contains("watermark") {
            return true;
        }
    }
    // FreeText + 打印但隐藏
    if subtype == b"FreeText" {
        let flags = dict.get(b"F").and_then(|v| v.as_i64()).unwrap_or(0);
        if flags & 4 != 0 && (flags & 2 != 0 || flags & 64 != 0) {
            return true;
        }
    }
    // 关键词匹配：Contents / T / QB_WM_MODEL 等字段
    if !keywords.is_empty() {
        let contents = dict
            .get(b"Contents")
            .ok()
            .and_then(|v| lopdf::decode_text_string(v).ok())
            .unwrap_or_default()
            .to_lowercase();
        let t = dict
            .get(b"T")
            .ok()
            .and_then(|v| lopdf::decode_text_string(v).ok())
            .unwrap_or_default()
            .to_lowercase();
        for kw in keywords {
            let kl = kw.to_lowercase();
            if contents.contains(&kl) || t.contains(&kl) {
                return true;
            }
        }
    }
    // 检查私有扩展字段（如 QB_WM_MODEL）是否含水印标识
    for key in &[b"QB_WM_MODEL".as_ref(), b"WatermarkModel", b"WMModel"] {
        if dict.get(key).is_ok() {
            return true;
        }
    }
    false
}

fn remove_annot_ref_from_page(doc: &mut Document, page_id: ObjectId, target: ObjectId) {
    if let Ok(d) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut())
        && let Ok(arr) = d.get_mut(b"Annots").and_then(|o| o.as_array_mut())
    {
        arr.retain(|o| o.as_reference().ok() != Some(target));
    }
}

fn delete_annot_object(doc: &mut Document, ann_id: ObjectId) {
    let ap_ids: Vec<ObjectId> = doc
        .get_object(ann_id)
        .and_then(|o| o.as_dict())
        .ok()
        .and_then(|d| d.get(b"AP").and_then(|o| o.as_dict()).ok())
        .map(|ap| {
            ap.iter()
                .flat_map(|(_, v)| {
                    if let Ok(id) = v.as_reference() {
                        vec![id]
                    } else if let Ok(d) = v.as_dict() {
                        d.iter()
                            .filter_map(|(_, vv)| vv.as_reference().ok())
                            .collect()
                    } else {
                        vec![]
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for id in ap_ids {
        doc.objects.remove(&id);
    }
    doc.objects.remove(&ann_id);
}

// ═══════════════════════════════════════════════════════════════
// ② 表单水印
// ═══════════════════════════════════════════════════════════════

fn remove_form_watermarks(
    doc: &mut Document,
    keywords: &[String],
    report: &mut Report,
) -> anyhow::Result<()> {
    let root_id = match doc.trailer.get(b"Root").and_then(|o| o.as_reference()) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };

    let acro_id_opt: Option<ObjectId> = doc
        .get_object(root_id)
        .and_then(|o| o.as_dict())
        .ok()
        .and_then(|d| d.get(b"AcroForm").and_then(|o| o.as_reference()).ok());

    let acro_dict_clone: Option<Dictionary> = {
        let root = match doc.get_object(root_id).and_then(|o| o.as_dict()) {
            Ok(d) => d.clone(),
            Err(_) => return Ok(()),
        };
        match root.get(b"AcroForm") {
            Ok(acro_obj) => {
                if let Ok(id) = acro_obj.as_reference() {
                    doc.get_object(id).and_then(|o| o.as_dict()).ok().cloned()
                } else {
                    acro_obj.as_dict().ok().cloned()
                }
            }
            Err(_) => return Ok(()),
        }
    };

    let acro_dict = match acro_dict_clone {
        Some(d) => d,
        None => return Ok(()),
    };

    let all_field_ids = collect_all_field_ids(doc, &acro_dict)?;
    let page_annot_map = build_page_annot_map(doc)?;

    let effective_kw: Vec<String> = if keywords.is_empty() {
        vec![
            "watermark".into(),
            "水印".into(),
            "draft".into(),
            "草稿".into(),
            "confidential".into(),
            "机密".into(),
            "sample".into(),
            "示例".into(),
        ]
    } else {
        keywords.to_vec()
    };

    let mut wm_ids: Vec<ObjectId> = Vec::new();
    for fid in &all_field_ids {
        if is_watermark_field(doc, *fid, &effective_kw) {
            wm_ids.push(*fid);
        }
    }

    for fid in &wm_ids {
        if let Some(&pg_id) = page_annot_map.get(fid) {
            remove_annot_ref_from_page(doc, pg_id, *fid);
        }
        delete_annot_object(doc, *fid);
        report.add(format!("[表单水印] 字段对象 {} {} R", fid.0, fid.1));
    }

    if !wm_ids.is_empty() {
        let wm_set: HashSet<ObjectId> = wm_ids.iter().cloned().collect();
        if let Some(acro_id) = acro_id_opt {
            if let Ok(acro) = doc.get_object_mut(acro_id).and_then(|o| o.as_dict_mut())
                && let Ok(fields) = acro.get_mut(b"Fields").and_then(|o| o.as_array_mut())
            {
                fields.retain(|o| {
                    o.as_reference()
                        .map(|id| !wm_set.contains(&id))
                        .unwrap_or(true)
                });
            }
        } else if let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
            && let Ok(acro) = root.get_mut(b"AcroForm").and_then(|o| o.as_dict_mut())
            && let Ok(fields) = acro.get_mut(b"Fields").and_then(|o| o.as_array_mut())
        {
            fields.retain(|o| {
                o.as_reference()
                    .map(|id| !wm_set.contains(&id))
                    .unwrap_or(true)
            });
        }
    }
    Ok(())
}

fn is_watermark_field(doc: &Document, fid: ObjectId, keywords: &[String]) -> bool {
    let dict = match doc.get_object(fid).and_then(|o| o.as_dict()) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let t = dict
        .get(b"T")
        .ok()
        .and_then(|v| lopdf::decode_text_string(v).ok())
        .unwrap_or_default()
        .to_lowercase();
    let tu = dict
        .get(b"TU")
        .ok()
        .and_then(|v| lopdf::decode_text_string(v).ok())
        .unwrap_or_default()
        .to_lowercase();
    for kw in keywords {
        let kl = kw.to_lowercase();
        if t.contains(&kl) || tu.contains(&kl) {
            return true;
        }
    }
    false
}

fn collect_all_field_ids(doc: &Document, acro_dict: &Dictionary) -> anyhow::Result<Vec<ObjectId>> {
    let mut ids = Vec::new();
    let fields_obj = match acro_dict.get(b"Fields") {
        Ok(o) => o,
        Err(_) => return Ok(ids),
    };
    let arr = match fields_obj {
        Object::Array(a) => a.clone(),
        Object::Reference(r) => match doc.get_object(*r).and_then(|o| o.as_array()) {
            Ok(a) => a.clone(),
            Err(_) => return Ok(ids),
        },
        _ => return Ok(ids),
    };
    for item in &arr {
        if let Ok(id) = item.as_reference() {
            ids.push(id);
            if let Ok(child_dict) = doc.get_object(id).and_then(|o| o.as_dict())
                && let Ok(kids) = child_dict.get(b"Kids").and_then(|o| o.as_array())
            {
                for k in kids {
                    if let Ok(kid_id) = k.as_reference() {
                        ids.push(kid_id);
                    }
                }
            }
        }
    }
    Ok(ids)
}

fn build_page_annot_map(doc: &Document) -> anyhow::Result<HashMap<ObjectId, ObjectId>> {
    let mut map = HashMap::new();
    for &page_id in doc.get_pages().values() {
        if let Ok(pd) = doc.get_object(page_id).and_then(|o| o.as_dict())
            && let Ok(annots) = pd.get(b"Annots").and_then(|o| o.as_array())
        {
            for ann in annots {
                if let Ok(aid) = ann.as_reference() {
                    map.insert(aid, page_id);
                }
            }
        }
    }
    Ok(map)
}

// ═══════════════════════════════════════════════════════════════
// 辅助：收集页面 ExtGState 透明度信息
// ═══════════════════════════════════════════════════════════════

/// gs_name -> (ca填充透明度, CA描边透明度)
fn collect_extgstate_opacity(doc: &Document, page_id: ObjectId) -> HashMap<String, (f32, f32)> {
    let mut map = HashMap::new();
    let resources = match crate::util::resolve_resources(doc, page_id) {
        Ok(r) => r,
        Err(_) => return map,
    };
    let ext_gs = match resources.get(b"ExtGState").and_then(|o| o.as_dict()) {
        Ok(d) => d.clone(),
        Err(_) => return map,
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
        let ca = gs_dict.get(b"ca").and_then(|v| v.as_f32()).unwrap_or(1.0);
        let ca_s = gs_dict.get(b"CA").and_then(|v| v.as_f32()).unwrap_or(1.0);
        map.insert(String::from_utf8_lossy(name).to_string(), (ca, ca_s));
    }
    map
}

// ═══════════════════════════════════════════════════════════════
// 辅助：收集页面 XObject 元信息
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct XObjInfo {
    is_image: bool,
    is_form: bool,
    has_smask: bool,
    /// 图片是否有 /ImageMask true（单色遮罩图，通常是印章/水印）
    has_image_mask: bool,
    /// Form XObject /PieceInfo 中是否含 /Watermark 标记
    piece_is_wm: bool,
    /// Form XObject 通过 /OC 引用的 OCG，OCG 名是否含水印关键词
    oc_is_wm: bool,
    /// Form XObject 的 /Name 字段值（部分生成器写入 "Watermark"）
    xobj_name: String,
    /// Form XObject 内容流预览（前 512 字节，用于关键词检测）
    content_preview: Vec<u8>,
}

fn collect_xobj_info(doc: &Document, page_id: ObjectId) -> HashMap<String, XObjInfo> {
    let mut map = HashMap::new();
    let resources = match crate::util::resolve_resources(doc, page_id) {
        Ok(r) => r,
        Err(_) => return map,
    };
    let xobjs = match resources.get(b"XObject").and_then(|o| o.as_dict()) {
        Ok(d) => d.clone(),
        Err(_) => return map,
    };
    for (res_name, val) in &xobjs {
        let id = match val.as_reference() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let stream = match doc.get_object(id).and_then(|o| o.as_stream()) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(|v| v.as_name())
            .unwrap_or(b"");
        let is_image = subtype == b"Image";
        let is_form = subtype == b"Form";
        let has_smask = stream.dict.get(b"SMask").is_ok();
        let has_image_mask = stream
            .dict
            .get(b"ImageMask")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 检查 /PieceInfo -> /ADBE_CompoundType -> /Private == /Watermark
        let piece_is_wm = check_piece_info_watermark(&stream.dict);

        // 检查 /OC -> OCG -> /Name 含水印关键词
        let oc_is_wm = check_oc_watermark(doc, &stream.dict);

        let xobj_name = stream
            .dict
            .get(b"Name")
            .ok()
            .and_then(|v| v.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_default();

        let content_preview = if is_form {
            stream
                .decompressed_content()
                .unwrap_or_default()
                .into_iter()
                .take(512)
                .collect()
        } else {
            vec![]
        };

        map.insert(
            String::from_utf8_lossy(res_name).to_string(),
            XObjInfo {
                is_image,
                is_form,
                has_smask,
                has_image_mask,
                piece_is_wm,
                oc_is_wm,
                xobj_name,
                content_preview,
            },
        );
    }
    map
}

/// 检查 stream dict 的 /PieceInfo 是否含 /Watermark 私有标记
///
/// 结构：/PieceInfo <</ADBE_CompoundType <</Private /Watermark ...>>>>
fn check_piece_info_watermark(dict: &Dictionary) -> bool {
    let pi = match dict.get(b"PieceInfo").and_then(|v| v.as_dict()) {
        Ok(d) => d,
        Err(_) => return false,
    };
    // 遍历所有条目，查找含 /Private /Watermark 的子字典
    for (_, v) in pi {
        if let Ok(sub) = v.as_dict()
            && let Ok(priv_val) = sub.get(b"Private")
            && let Ok(name) = priv_val.as_name()
            && name == b"Watermark"
        {
            return true;
        }
    }
    false
}

/// 检查 stream dict 的 /OC -> /OCGs 引用的 OCG 名称是否含 "Watermark"
fn check_oc_watermark(doc: &Document, dict: &Dictionary) -> bool {
    // /OC 可以是 OCMD (/OCGs) 或直接的 OCG
    let oc_obj = match dict.get(b"OC") {
        Ok(o) => o,
        Err(_) => return false,
    };

    // 尝试解引用
    let oc_dict = if let Ok(id) = oc_obj.as_reference() {
        match doc.get_object(id).and_then(|o| o.as_dict()) {
            Ok(d) => d,
            Err(_) => return false,
        }
    } else {
        match oc_obj.as_dict() {
            Ok(d) => d,
            Err(_) => return false,
        }
    };

    // 直接是 OCG：检查 /Name
    if check_ocg_name_watermark(oc_dict) {
        return true;
    }

    // 是 OCMD：遍历 /OCGs 数组
    if let Ok(ocgs) = oc_dict.get(b"OCGs").and_then(|v| v.as_array()) {
        for item in ocgs {
            if let Ok(ocg_id) = item.as_reference()
                && let Ok(ocg_dict) = doc.get_object(ocg_id).and_then(|o| o.as_dict())
                && check_ocg_name_watermark(ocg_dict)
            {
                return true;
            }
        }
    }
    false
}

fn check_ocg_name_watermark(dict: &Dictionary) -> bool {
    let name = match dict.get(b"Name") {
        Ok(n) => n,
        Err(_) => return false,
    };
    // /Name 可能是字符串或 Name 对象
    let name_str = lopdf::decode_text_string(name).unwrap_or_else(|_| {
        name.as_name()
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_default()
    });
    name_str.to_lowercase().contains("watermark")
}

// ═══════════════════════════════════════════════════════════════
// ③–⑧ 内容流级水印
// ═══════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
fn remove_content_watermarks(
    doc: &mut Document,
    pages: &[u32],
    do_text: bool,
    do_curve: bool,
    do_path: bool,
    do_trace: bool,
    do_image: bool,
    do_pattern: bool,
    opacity_threshold: f32,
    _keywords: &[String],
    report: &mut Report,
) -> anyhow::Result<()> {
    let page_ids: Vec<(u32, ObjectId)> = {
        let pm = doc.get_pages();
        pages
            .iter()
            .filter_map(|p| pm.get(p).map(|id| (*p, *id)))
            .collect()
    };

    for (pg_num, page_id) in page_ids {
        let gs_map = collect_extgstate_opacity(doc, page_id);
        let xobj_map = collect_xobj_info(doc, page_id);

        let raw = match doc.get_page_content(page_id) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let content = match lopdf::content::Content::decode(&raw) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let ops = content.operations;
        let n = ops.len();
        let mut to_delete: HashSet<usize> = HashSet::new();

        // ── 第一遍：扫描 BDC /Artifact <</Subtype /Watermark>> … EMC 块 ──
        // 这是最可靠的水印标记，适用于注释水印、图案水印、文本水印
        {
            let mut i = 0;
            while i < n {
                if ops[i].operator == "BDC"
                    && let Some((tag, subtype)) = parse_bdc_subtype(&ops[i])
                {
                    let is_wm_artifact = tag == "Artifact" && subtype == "Watermark";
                    if is_wm_artifact {
                        // 找对应的 EMC，删除整个 BDC…EMC 块
                        let bdc_start = i;
                        let mut depth = 1i32;
                        i += 1;
                        while i < n && depth > 0 {
                            match ops[i].operator.as_str() {
                                "BDC" | "BMC" => depth += 1,
                                "EMC" => depth -= 1,
                                _ => {}
                            }
                            i += 1;
                        }
                        let bdc_end = i - 1; // EMC 的位置
                        // 判断类型并决定是否删除
                        let inner = &ops[bdc_start..=bdc_end];
                        let has_do = inner.iter().any(|o| o.operator == "Do");
                        let has_text = inner
                            .iter()
                            .any(|o| matches!(o.operator.as_str(), "Tj" | "TJ" | "'" | "\""));
                        let has_path = inner.iter().any(|o| {
                            matches!(
                                o.operator.as_str(),
                                "m" | "l"
                                    | "c"
                                    | "v"
                                    | "y"
                                    | "h"
                                    | "re"
                                    | "S"
                                    | "s"
                                    | "f"
                                    | "F"
                                    | "f*"
                                    | "B"
                                    | "B*"
                                    | "b"
                                    | "b*"
                                    | "n"
                            )
                        });

                        // 识别 Do 的 XObject 类型
                        let do_xobj_name: Option<String> = inner
                            .iter()
                            .find(|o| o.operator == "Do")
                            .and_then(|o| o.operands.first())
                            .and_then(|o| o.as_name().ok())
                            .map(|n| String::from_utf8_lossy(n).to_string());

                        let xobj_is_form = do_xobj_name
                            .as_ref()
                            .and_then(|nm| xobj_map.get(nm.as_str()))
                            .map(|xi| xi.is_form)
                            .unwrap_or(false);
                        let xobj_is_image = do_xobj_name
                            .as_ref()
                            .and_then(|nm| xobj_map.get(nm.as_str()))
                            .map(|xi| xi.is_image)
                            .unwrap_or(false);

                        let should_del = if has_do && xobj_is_form && do_pattern {
                            let xi = do_xobj_name
                                .as_ref()
                                .and_then(|nm| xobj_map.get(nm.as_str()))
                                .unwrap();
                            report.add(format!(
                                "[图案水印] 第{}页 BDC-Watermark/Form /{} ops #{}-#{}",
                                pg_num, xi.xobj_name, bdc_start, bdc_end
                            ));
                            true
                        } else if has_do && xobj_is_image && do_image {
                            report.add(format!(
                                "[图片水印] 第{}页 BDC-Watermark/Image ops #{}-#{}",
                                pg_num, bdc_start, bdc_end
                            ));
                            true
                        } else if has_text && do_text {
                            report.add(format!(
                                "[文本水印] 第{}页 BDC-Watermark/Text ops #{}-#{}",
                                pg_num, bdc_start, bdc_end
                            ));
                            true
                        } else if has_path && !has_text && !has_do {
                            if do_path {
                                report.add(format!(
                                    "[路径水印] 第{}页 BDC-Watermark/Path ops #{}-#{}",
                                    pg_num, bdc_start, bdc_end
                                ));
                                true
                            } else {
                                false
                            }
                        } else if !has_do && !has_text && !has_path {
                            // 纯 gs/cm 层（痕迹）
                            if do_trace {
                                report.add(format!(
                                    "[痕迹水印] 第{}页 BDC-Watermark ops #{}-#{}",
                                    pg_num, bdc_start, bdc_end
                                ));
                                true
                            } else {
                                false
                            }
                        } else {
                            // 兜底：只要是 Watermark artifact 就删
                            report.add(format!(
                                "[水印] 第{}页 BDC-Artifact/Watermark ops #{}-#{}",
                                pg_num, bdc_start, bdc_end
                            ));
                            true
                        };

                        if should_del {
                            for j in bdc_start..=bdc_end {
                                to_delete.insert(j);
                            }
                        }
                        continue; // i 已经推进
                    }
                }
                i += 1;
            }
        }

        // ── 第二遍：扫描 q…Q 块内的水印模式 ──────────────────────
        // 只检测叶子级或浅层的 q…Q 块（跨度 <= 200 个操作符），
        // 避免把整篇文档的顶层 q…Q 误判为水印块。
        {
            let mut i = 0;
            while i < n {
                if ops[i].operator != "q" {
                    i += 1;
                    continue;
                }
                // 找配对的 Q
                let q_start = i;
                let mut depth = 1i32;
                i += 1;
                while i < n && depth > 0 {
                    match ops[i].operator.as_str() {
                        "q" => depth += 1,
                        "Q" => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                let q_end = i - 1;
                let q_span = q_end - q_start + 1;

                // 如果已经整块被删除，跳过
                if to_delete.contains(&q_start) {
                    continue;
                }

                // 跨度超过 200 个操作符的顶层大块不做水印整体判断，
                // 其内部的水印子块会在后续迭代中被单独处理。
                if q_span > 200 {
                    continue;
                }

                let inner = &ops[q_start..=q_end];

                // 收集本块内所有 gs 引用的最低透明度
                let min_opacity = inner
                    .iter()
                    .filter(|o| o.operator == "gs")
                    .filter_map(|o| o.operands.first().and_then(|v| v.as_name().ok()))
                    .filter_map(|n| gs_map.get(String::from_utf8_lossy(n).as_ref()))
                    .map(|&(ca, ca_s)| ca.min(ca_s))
                    .fold(1.0f32, f32::min);

                let has_low_opacity = min_opacity < opacity_threshold;

                // 检查 q…Q 块内是否有旋转/倾斜的 cm 矩阵
                // cm: a b c d e f — 若 b 或 c 非零，说明坐标系有旋转
                let has_cm_rotate = inner.iter().any(|o| {
                    if o.operator != "cm" || o.operands.len() < 6 {
                        return false;
                    }
                    let b = o
                        .operands
                        .get(1)
                        .and_then(|v| v.as_f32().ok())
                        .unwrap_or(0.0);
                    let c = o
                        .operands
                        .get(2)
                        .and_then(|v| v.as_f32().ok())
                        .unwrap_or(0.0);
                    b.abs() > 0.01 || c.abs() > 0.01
                });

                // 收集 Do 的 XObject 信息
                let do_names: Vec<String> = inner
                    .iter()
                    .filter(|o| o.operator == "Do")
                    .filter_map(|o| o.operands.first().and_then(|v| v.as_name().ok()))
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .collect();

                let has_bezier = inner
                    .iter()
                    .any(|o| matches!(o.operator.as_str(), "c" | "v" | "y"));
                let has_path_paint = inner.iter().any(|o| {
                    matches!(
                        o.operator.as_str(),
                        "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                    )
                });
                let has_text_show = inner
                    .iter()
                    .any(|o| matches!(o.operator.as_str(), "Tj" | "TJ" | "'" | "\""));
                let has_tm_rotate = inner.iter().any(|o| {
                    if o.operator != "Tm" || o.operands.len() < 6 {
                        return false;
                    }
                    // Tm: a b c d e f — 若 b 或 c 非零，说明有旋转/倾斜
                    let b = o
                        .operands
                        .get(1)
                        .and_then(|v| v.as_f32().ok())
                        .unwrap_or(0.0);
                    let c = o
                        .operands
                        .get(2)
                        .and_then(|v| v.as_f32().ok())
                        .unwrap_or(0.0);
                    b.abs() > 0.01 || c.abs() > 0.01
                });
                let has_tr_invisible = inner.iter().any(|o| {
                    o.operator == "Tr"
                        && o.operands
                            .first()
                            .and_then(|v| v.as_i64().ok())
                            .unwrap_or(0)
                            == 7
                });

                // ── ③ 文本水印：q…Q 内有 BT…ET，旋转矩阵 + 低透明度 ──
                // 识别条件（满足任一）：
                //   a. gs 引用低透明度 ExtGState（ca < threshold）
                //   b. Tm 矩阵含旋转分量（b/c 非零）
                //   c. cm 矩阵含旋转分量（b/c 非零）—— 用坐标系旋转实现的水印
                //   d. Tr 7 不可见渲染模式
                if do_text
                    && has_text_show
                    && (has_low_opacity || has_tm_rotate || has_cm_rotate || has_tr_invisible)
                {
                    report.add(format!(
                        "[文本水印] 第{}页 q-Text ops #{}-#{}",
                        pg_num, q_start, q_end
                    ));
                    for j in q_start..=q_end {
                        to_delete.insert(j);
                    }
                    continue;
                }

                // ── ④ 曲线水印：大量贝塞尔曲线 + 低透明度 ───────────
                if do_curve && has_bezier && has_path_paint {
                    let curve_count = inner
                        .iter()
                        .filter(|o| matches!(o.operator.as_str(), "c" | "v" | "y"))
                        .count();
                    if has_low_opacity || curve_count >= 8 {
                        report.add(format!(
                            "[曲线水印] 第{}页 q-Curve({} segs) ops #{}-#{}",
                            pg_num, curve_count, q_start, q_end
                        ));
                        for j in q_start..=q_end {
                            to_delete.insert(j);
                        }
                        continue;
                    }
                }

                // ── ⑤ 路径水印：路径填充/描边 + 低透明度 ────────────
                if do_path && !has_text_show && !has_bezier && has_path_paint && has_low_opacity {
                    report.add(format!(
                        "[路径水印] 第{}页 q-Path ops #{}-#{}",
                        pg_num, q_start, q_end
                    ));
                    for j in q_start..=q_end {
                        to_delete.insert(j);
                    }
                    continue;
                }

                // ── ⑥ 痕迹水印：仅 gs + cm/Do，极低透明度 ──────────
                if do_trace
                    && !has_text_show
                    && !has_path_paint
                    && !has_bezier
                    && has_low_opacity
                    && min_opacity < 0.3
                {
                    report.add(format!(
                        "[痕迹水印] 第{}页 q-Trace(ca={:.2}) ops #{}-#{}",
                        pg_num, min_opacity, q_start, q_end
                    ));
                    for j in q_start..=q_end {
                        to_delete.insert(j);
                    }
                    continue;
                }

                // ── ⑦ 图片水印：q-cm-Do(Image with SMask/ImageMask)-Q ──
                if do_image && !do_names.is_empty() {
                    for xname in &do_names {
                        if let Some(xi) = xobj_map.get(xname.as_str())
                            && xi.is_image
                            && (xi.has_smask || xi.has_image_mask)
                        {
                            let kind = if xi.has_smask { "SMask" } else { "ImageMask" };
                            report.add(format!(
                                "[图片水印] 第{}页 q-Do-Image/{} ({}) ops #{}-#{}",
                                pg_num, xname, kind, q_start, q_end
                            ));
                            for j in q_start..=q_end {
                                to_delete.insert(j);
                            }
                            break;
                        }
                    }
                    if to_delete.contains(&q_start) {
                        continue;
                    }
                }

                // ── ⑧ 图案水印：q-gs-cm-Do(Form/Watermark)-Q ───────
                if do_pattern && !do_names.is_empty() {
                    for xname in &do_names {
                        if let Some(xi) = xobj_map.get(xname.as_str())
                            && xi.is_form
                            && (xi.piece_is_wm
                                || xi.oc_is_wm
                                || xi.xobj_name.to_lowercase().contains("watermark")
                                || (has_low_opacity
                                    && is_form_content_watermark(&xi.content_preview)))
                        {
                            report.add(format!(
                                "[图案水印] 第{}页 q-Do-Form/{} ops #{}-#{}",
                                pg_num, xname, q_start, q_end
                            ));
                            for j in q_start..=q_end {
                                to_delete.insert(j);
                            }
                            break;
                        }
                    }
                }
            }
        }

        if to_delete.is_empty() {
            continue;
        }

        // 重建内容流
        let new_ops: Vec<lopdf::content::Operation> = ops
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !to_delete.contains(i))
            .map(|(_, op)| op)
            .collect();

        match (lopdf::content::Content {
            operations: new_ops,
        })
        .encode()
        {
            Ok(encoded) => {
                if let Err(e) = doc.change_page_content(page_id, encoded) {
                    eprintln!("⚠️  第{}页内容流写入失败: {}", pg_num, e);
                }
            }
            Err(e) => eprintln!("⚠️  第{}页内容流重编码失败: {}", pg_num, e),
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 辅助：判断 Form XObject 内容流是否像水印
// ═══════════════════════════════════════════════════════════════

/// 检查 Form XObject 内容流预览是否含水印特征文字
fn is_form_content_watermark(preview: &[u8]) -> bool {
    let s = String::from_utf8_lossy(preview).to_lowercase();
    const WM_KW: &[&str] = &["watermark", "水印", "draft", "草稿", "confidential", "机密"];
    WM_KW.iter().any(|kw| s.contains(kw))
}

// ═══════════════════════════════════════════════════════════════
// ⑨ 内容流搜索水印
// ═══════════════════════════════════════════════════════════════

/// 搜索模式
enum SearchPattern {
    /// 字节串匹配：匹配 Tj/TJ 字符串操作数中包含这些字节的操作
    Bytes(Vec<u8>),
    /// 操作符模式：(name_glob, operator)，如 ("KSPX*", "Do")
    Operator(String, String),
}

/// 解析搜索模式字符串（以 | 分隔多个模式）
fn parse_search_patterns(s: &str) -> Vec<SearchPattern> {
    s.split('|')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(parse_single_pattern)
        .collect()
}

fn parse_single_pattern(s: &str) -> SearchPattern {
    // 检测操作符模式："/Name Op" 或 "/Name* Op"
    if let Some(sp) = s.rfind(' ') {
        let name_part = s[..sp].trim();
        let op_part = s[sp + 1..].trim();
        if name_part.starts_with('/')
            && name_part.len() > 1
            && !op_part.is_empty()
            && op_part.chars().all(|c| c.is_ascii_alphabetic())
        {
            return SearchPattern::Operator(name_part[1..].to_string(), op_part.to_string());
        }
    }
    // 其余情况作为字节串处理
    // 以 0x / 0X 开头：保留原始字符串作为字节（ASCII），同时也尝试十六进制解码
    // 无前缀：视为十六进制串，解码后作为字节
    let hex_str = if s.starts_with("0x") || s.starts_with("0X") {
        &s[2..]
    } else {
        s
    };
    if let Some(decoded) = hex_decode(hex_str) {
        SearchPattern::Bytes(decoded)
    } else {
        SearchPattern::Bytes(s.as_bytes().to_vec())
    }
}

/// 十六进制字符串 → 字节数组
fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

/// 简单 glob 匹配（支持 * 和 ?），使用动态规划避免栈溢出
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (m, n) = (p.len(), t.len());

    // dp[i][j] = pattern[..i] 是否匹配 text[..j]
    let mut dp = vec![vec![false; n + 1]; m + 1];
    dp[0][0] = true;

    // pattern 开头的连续 * 可以匹配空串
    for i in 1..=m {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        } else {
            break;
        }
    }

    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == t[j - 1],
            };
        }
    }

    dp[m][n]
}

/// 检查操作是否匹配某个搜索模式
fn match_op_pattern(op: &lopdf::content::Operation, pat: &SearchPattern) -> bool {
    match pat {
        SearchPattern::Operator(name_glob, operator) => {
            if op.operator != *operator {
                return false;
            }
            op.operands
                .first()
                .and_then(|o| o.as_name().ok())
                .map(|n| glob_match(name_glob, &String::from_utf8_lossy(n)))
                .unwrap_or(false)
        }
        SearchPattern::Bytes(needle) => {
            if !matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
                return false;
            }
            op.operands.iter().any(|o| match o {
                lopdf::Object::String(s, _) => bytes_contain(s, needle),
                lopdf::Object::Array(arr) => arr.iter().any(|item| {
                    if let lopdf::Object::String(s, _) = item {
                        bytes_contain(s, needle)
                    } else {
                        false
                    }
                }),
                _ => false,
            })
        }
    }
}

/// 判断 haystack 中是否包含 needle
fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// 找到包含 target 操作的最内层 q…Q 范围（包含端点）
fn find_enclosing_q_q(ops: &[lopdf::content::Operation], target: usize) -> Option<(usize, usize)> {
    // 向前找最近未匹配的 q
    let mut depth = 0i32;
    let mut q_start = None;
    for j in (0..target).rev() {
        match ops[j].operator.as_str() {
            "Q" => depth += 1,
            "q" => {
                if depth == 0 {
                    q_start = Some(j);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let q_start = q_start?;
    // 向后找配对的 Q
    let mut depth = 0i32;
    for (k, op) in ops.iter().enumerate().skip(q_start + 1) {
        match op.operator.as_str() {
            "q" => depth += 1,
            "Q" => {
                if depth == 0 {
                    return Some((q_start, k));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// 找到包含 target 操作的 BT…ET 范围
fn find_enclosing_btet(ops: &[lopdf::content::Operation], target: usize) -> Option<(usize, usize)> {
    let mut bt_start = None;
    for j in (0..target).rev() {
        match ops[j].operator.as_str() {
            "BT" => {
                bt_start = Some(j);
                break;
            }
            "ET" => break,
            _ => {}
        }
    }
    let bt_start = bt_start?;
    for (k, op) in ops.iter().enumerate().skip(bt_start + 1) {
        if op.operator == "ET" {
            return Some((bt_start, k));
        }
    }
    None
}

/// 确定删除 target 时应删除的范围：优先 q…Q，其次 BT…ET，最后仅本操作
fn block_to_delete(ops: &[lopdf::content::Operation], target: usize) -> (usize, usize) {
    if let Some(r) = find_enclosing_q_q(ops, target) {
        return r;
    }
    if matches!(ops[target].operator.as_str(), "Tj" | "TJ" | "'" | "\"")
        && let Some(r) = find_enclosing_btet(ops, target)
    {
        return r;
    }
    (target, target)
}

/// 收集页面 Form XObject 中内容流匹配搜索模式的资源名称
fn collect_resource_stream_matches(
    doc: &Document,
    page_id: ObjectId,
    patterns: &[SearchPattern],
) -> std::collections::HashSet<String> {
    let mut matched = std::collections::HashSet::new();
    let resources = match crate::util::resolve_resources(doc, page_id) {
        Ok(r) => r,
        Err(_) => return matched,
    };
    let xobjs = match resources.get(b"XObject").and_then(|o| o.as_dict()) {
        Ok(d) => d.clone(),
        Err(_) => return matched,
    };
    for (res_name, val) in &xobjs {
        let id = match val.as_reference() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let stream = match doc.get_object(id).and_then(|o| o.as_stream()) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(|v| v.as_name())
            .unwrap_or(b"");
        if subtype != b"Form" {
            continue;
        }
        let content_bytes = match stream.decompressed_content() {
            Ok(b) => b,
            Err(_) => continue,
        };
        // 原始字节搜索（捕获字面量匹配）
        'outer: for pat in patterns {
            if let SearchPattern::Bytes(needle) = pat {
                // 在原始流字节中搜索（覆盖 <hex> 和 (string) 两种格式）
                if bytes_contain(&content_bytes, needle) {
                    matched.insert(String::from_utf8_lossy(res_name).to_string());
                    break 'outer;
                }
            }
        }
        // 解析操作搜索
        if let Ok(content) = lopdf::content::Content::decode(&content_bytes) {
            'op_outer: for op in &content.operations {
                for pat in patterns {
                    if match_op_pattern(op, pat) {
                        matched.insert(String::from_utf8_lossy(res_name).to_string());
                        break 'op_outer;
                    }
                }
            }
        }
    }
    matched
}

/// 删除内容流搜索匹配到的水印块
fn remove_stream_search_watermarks(
    doc: &mut Document,
    pages: &[u32],
    search_resource_streams: bool,
    patterns_str: &str,
    report: &mut Report,
) -> anyhow::Result<()> {
    let patterns = parse_search_patterns(patterns_str);
    if patterns.is_empty() {
        return Ok(());
    }

    let page_ids: Vec<(u32, ObjectId)> = {
        let pm = doc.get_pages();
        pages
            .iter()
            .filter_map(|p| pm.get(p).map(|id| (*p, *id)))
            .collect()
    };

    for (pg_num, page_id) in page_ids {
        // 收集资源流中匹配的 XObject 名称
        let res_matched: std::collections::HashSet<String> = if search_resource_streams {
            collect_resource_stream_matches(doc, page_id, &patterns)
        } else {
            std::collections::HashSet::new()
        };

        let raw = match doc.get_page_content(page_id) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let content = match lopdf::content::Content::decode(&raw) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let ops = content.operations;
        let n = ops.len();
        let mut to_delete: HashSet<usize> = HashSet::new();

        for i in 0..n {
            if to_delete.contains(&i) {
                continue;
            }
            let op = &ops[i];

            // 直接模式匹配
            let direct_match = patterns.iter().any(|p| match_op_pattern(op, p));

            // 资源流中匹配的 XObject 对应的 Do 调用
            let res_match = !res_matched.is_empty()
                && op.operator == "Do"
                && op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .map(|n| res_matched.contains(String::from_utf8_lossy(n).as_ref()))
                    .unwrap_or(false);

            if direct_match || res_match {
                let (start, end) = block_to_delete(&ops, i);
                let kind = if res_match {
                    "资源内容搜索"
                } else {
                    "内容搜索"
                };
                report.add(format!("[{}] 第{}页 ops #{}-#{}", kind, pg_num, start, end));
                for j in start..=end {
                    to_delete.insert(j);
                }
            }
        }

        if to_delete.is_empty() {
            continue;
        }

        let new_ops: Vec<lopdf::content::Operation> = ops
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !to_delete.contains(i))
            .map(|(_, op)| op)
            .collect();

        match (lopdf::content::Content {
            operations: new_ops,
        })
        .encode()
        {
            Ok(encoded) => {
                if let Err(e) = doc.change_page_content(page_id, encoded) {
                    eprintln!("⚠️  第{}页内容流写入失败: {}", pg_num, e);
                }
            }
            Err(e) => eprintln!("⚠️  第{}页内容流重编码失败: {}", pg_num, e),
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// ⑩ 资源序号删除水印
// ═══════════════════════════════════════════════════════════════

/// 解析序号规范，返回 0-based 索引列表
///
/// 格式：
///   "1,3-4"  → 正序第 1、3、4（1-based）
///   "-1"     → 倒序第 1（最后一个）
///   "-1-3"   → 倒序第 1 至第 3（最后三个）
fn parse_index_spec(spec: &str, total: usize) -> Vec<usize> {
    let mut result = Vec::new();
    for token in spec.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(rest) = token.strip_prefix('-') {
            // 倒序模式
            if let Some(dash_pos) = rest.find('-') {
                // "-A-B"：倒序范围 A..=B
                let a_str = &rest[..dash_pos];
                let b_str = &rest[dash_pos + 1..];
                if let (Ok(a), Ok(b)) = (a_str.parse::<usize>(), b_str.parse::<usize>()) {
                    for i in a..=b {
                        if i >= 1 && i <= total {
                            result.push(total - i);
                        }
                    }
                }
            } else if let Ok(n) = rest.parse::<usize>() {
                // "-N"：倒序第 N 个
                if n >= 1 && n <= total {
                    result.push(total - n);
                }
            }
        } else if let Some(dash_pos) = token.find('-') {
            // 正序范围 "A-B"
            let a_str = &token[..dash_pos];
            let b_str = &token[dash_pos + 1..];
            if let (Ok(a), Ok(b)) = (a_str.parse::<usize>(), b_str.parse::<usize>()) {
                for i in a..=b {
                    if i >= 1 && i <= total {
                        result.push(i - 1);
                    }
                }
            }
        } else if let Ok(n) = token.parse::<usize>() {
            // 正序单个
            if n >= 1 && n <= total {
                result.push(n - 1);
            }
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

/// 从页面的 Resources/XObject 字典中获取所有 Image 资源名（排序后）
fn collect_image_xobj_names(doc: &Document, page_id: ObjectId) -> Vec<String> {
    let resources = match crate::util::resolve_resources(doc, page_id) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let xobjs = match resources.get(b"XObject").and_then(|o| o.as_dict()) {
        Ok(d) => d.clone(),
        Err(_) => return vec![],
    };
    let mut pairs: Vec<(String, u32)> = xobjs
        .iter()
        .filter_map(|(name, val)| {
            let id = val.as_reference().ok()?;
            let s = doc.get_object(id).and_then(|o| o.as_stream()).ok()?;
            let is_image = s
                .dict
                .get(b"Subtype")
                .and_then(|v| v.as_name())
                .map(|n| n == b"Image")
                .unwrap_or(false);
            if is_image {
                Some((String::from_utf8_lossy(name).to_string(), id.0))
            } else {
                None
            }
        })
        .collect();
    // 按 ObjectId 升序排列：后加入 PDF 的资源（ID 更大）排在末尾，
    // 使 "-1-3" 等倒序索引能正确定位到最新插入的水印资源。
    pairs.sort_by_key(|(_, obj_num)| *obj_num);
    pairs.into_iter().map(|(name, _)| name).collect()
}

/// 从页面 Resources/XObject 字典中删除指定名称的资源
fn remove_xobj_from_resources(doc: &mut Document, page_id: ObjectId, names: &HashSet<String>) {
    // 判断 Resources 是直接字典还是间接引用
    let res_ref: Option<ObjectId> = doc
        .get_object(page_id)
        .and_then(|o| o.as_dict())
        .ok()
        .and_then(|d| d.get(b"Resources").and_then(|o| o.as_reference()).ok());

    if let Some(res_id) = res_ref {
        // Resources 是间接引用
        if let Ok(res_dict) = doc.get_object_mut(res_id).and_then(|o| o.as_dict_mut())
            && let Ok(xobj_dict) = res_dict.get_mut(b"XObject").and_then(|o| o.as_dict_mut())
        {
            for name in names {
                xobj_dict.remove(name.as_bytes());
            }
        }
    } else {
        // Resources 内联在页面字典中
        if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut())
            && let Ok(res_dict) = page_dict
                .get_mut(b"Resources")
                .and_then(|o| o.as_dict_mut())
            && let Ok(xobj_dict) = res_dict.get_mut(b"XObject").and_then(|o| o.as_dict_mut())
        {
            for name in names {
                xobj_dict.remove(name.as_bytes());
            }
        }
    }
}

/// 按序号删除图像资源水印
fn remove_res_index_watermarks(
    doc: &mut Document,
    pages: &[u32],
    del_spec: Option<&str>,
    skip_spec: Option<&str>,
    report: &mut Report,
) -> anyhow::Result<()> {
    let page_ids: Vec<(u32, ObjectId)> = {
        let pm = doc.get_pages();
        pages
            .iter()
            .filter_map(|p| pm.get(p).map(|id| (*p, *id)))
            .collect()
    };

    for (pg_num, page_id) in page_ids {
        let image_names = collect_image_xobj_names(doc, page_id);
        let total = image_names.len();
        if total == 0 {
            continue;
        }

        // 确定待删除索引集合
        let del_indices: HashSet<usize> = if let Some(spec) = del_spec {
            parse_index_spec(spec, total).into_iter().collect()
        } else {
            // 无 del_spec：默认删除全部（配合 skip 使用）
            (0..total).collect()
        };

        let skip_indices: HashSet<usize> = if let Some(spec) = skip_spec {
            parse_index_spec(spec, total).into_iter().collect()
        } else {
            HashSet::new()
        };

        let to_delete: HashSet<String> = image_names
            .iter()
            .enumerate()
            .filter(|(i, _)| del_indices.contains(i) && !skip_indices.contains(i))
            .map(|(_, name)| name.clone())
            .collect();

        if to_delete.is_empty() {
            continue;
        }

        // 1. 从内容流中删除对应的 Do 调用块
        if let Ok(raw) = doc.get_page_content(page_id)
            && let Ok(content) = lopdf::content::Content::decode(&raw)
        {
            let ops = content.operations;
            let n = ops.len();
            let mut del_ops: HashSet<usize> = HashSet::new();
            for i in 0..n {
                if ops[i].operator == "Do"
                    && let Some(name) = ops[i]
                        .operands
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .map(|n| String::from_utf8_lossy(n).to_string())
                    && to_delete.contains(&name)
                {
                    let (start, end) = block_to_delete(&ops, i);
                    for j in start..=end {
                        del_ops.insert(j);
                    }
                }
            }
            if !del_ops.is_empty() {
                let new_ops: Vec<lopdf::content::Operation> = ops
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| !del_ops.contains(i))
                    .map(|(_, op)| op)
                    .collect();
                if let Ok(encoded) = (lopdf::content::Content {
                    operations: new_ops,
                })
                .encode()
                    && let Err(e) = doc.change_page_content(page_id, encoded)
                {
                    eprintln!("⚠️  第{}页内容流写入失败: {}", pg_num, e);
                }
            }
        }

        // 2. 从 Resources/XObject 中删除
        remove_xobj_from_resources(doc, page_id, &to_delete);

        for name in &to_delete {
            report.add(format!("[资源序号水印] 第{}页 图像资源 /{}", pg_num, name));
        }
    }
    Ok(())
}
