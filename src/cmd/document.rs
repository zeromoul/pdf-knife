use lopdf::{Document, Object, ObjectId};
use std::path::PathBuf;

pub fn doc_info(input: PathBuf, password: &Option<String>) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    println!("\n--- 文档基本信息 ---");
    println!("  PDF 版本  : {}", doc.version);
    println!("  总页数    : {}", doc.get_pages().len());
    println!("  对象总数  : {}", doc.objects.len());
    println!(
        "  加密      : {}",
        if doc.trailer.get(b"Encrypt").is_ok() {
            "✅ 已加密"
        } else {
            "❌ 未加密"
        }
    );

    if let Ok(info_id) = doc.trailer.get(b"Info").and_then(|i| i.as_reference()) {
        if let Ok(info) = doc.get_object(info_id)?.as_dict() {
            println!("\n  [Info 字典]");
            for (k, v) in info {
                let key = String::from_utf8_lossy(k);
                let val = match v {
                    Object::String(s, _) => decode_pdf_string(s),
                    other => format!("{:?}", other),
                };
                println!("    /{:<16}: {}", key, val);
            }
        }
    } else {
        println!("  Info      : 无");
    }

    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|r| r.as_reference())
        && let Ok(root) = doc.get_object(root_id)?.as_dict()
    {
        println!("\n  [Catalog 摘要]");
        for key in &[
            b"PageLayout".as_ref(),
            b"PageMode",
            b"Lang",
            b"Metadata",
            b"Outlines",
            b"AcroForm",
            b"Permissions",
        ] {
            if let Ok(v) = root.get(key) {
                println!("    /{:<16}: {:?}", String::from_utf8_lossy(key), v);
            }
        }
    }
    Ok(())
}

pub fn outline(
    input: PathBuf,
    depth: Option<usize>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;
    let root = doc.get_object(root_id)?.as_dict()?;

    let outline_id = match root.get(b"Outlines").and_then(|o| o.as_reference()) {
        Ok(id) => id,
        Err(_) => {
            println!("该文档没有书签大纲。");
            return Ok(());
        }
    };
    let outline_dict = doc.get_object(outline_id)?.as_dict()?;
    let count = outline_dict
        .get(b"Count")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    println!("--- 书签大纲 (顶层 {} 项) ---", count);

    if let Ok(first_id) = outline_dict.get(b"First").and_then(|f| f.as_reference()) {
        print_outline_node(&doc, first_id, 0, depth)?;
    } else {
        println!("（大纲为空）");
    }
    Ok(())
}

fn print_outline_node(
    doc: &Document,
    node_id: ObjectId,
    depth: usize,
    max: Option<usize>,
) -> anyhow::Result<()> {
    if let Some(m) = max
        && depth > m
    {
        return Ok(());
    }
    let node = doc.get_object(node_id)?.as_dict()?;
    if let Ok(title) = node.get(b"Title").and_then(|t| t.as_str()) {
        let indent = "  ".repeat(depth);
        let title_str = if title.starts_with(&[0xFE, 0xFF]) {
            let utf16: Vec<u16> = title[2..]
                .chunks(2)
                .map(|c| ((c[0] as u16) << 8) | c.get(1).copied().unwrap_or(0) as u16)
                .collect();
            String::from_utf16_lossy(&utf16)
        } else {
            String::from_utf8_lossy(title).to_string()
        };
        // 尝试获取目标页
        let dest_page = get_dest_page(doc, node);
        if let Some(pg) = dest_page {
            println!("{}◆ {} → 第{}页", indent, title_str, pg);
        } else {
            println!("{}◆ {}", indent, title_str);
        }
    }
    if let Ok(first) = node.get(b"First").and_then(|f| f.as_reference()) {
        print_outline_node(doc, first, depth + 1, max)?;
    }
    if let Ok(next) = node.get(b"Next").and_then(|n| n.as_reference()) {
        print_outline_node(doc, next, depth, max)?;
    }
    Ok(())
}

/// 解码 PDF 字符串：UTF-16 BE BOM (FE FF) 自动转换，否则按 latin-1 / utf-8 呈现
fn decode_pdf_string(s: &[u8]) -> String {
    if s.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = s[2..]
            .chunks(2)
            .map(|c| ((c[0] as u16) << 8) | c.get(1).copied().unwrap_or(0) as u16)
            .collect();
        String::from_utf16_lossy(&utf16)
    } else {
        String::from_utf8_lossy(s).to_string()
    }
}

fn get_dest_page(doc: &Document, node: &lopdf::Dictionary) -> Option<u32> {
    // 方式1: /Dest 直接指定 [page_ref /Fit...]
    if let Ok(dest) = node.get(b"Dest") {
        let page_ref = match dest {
            Object::Array(arr) => arr.first()?.as_reference().ok()?,
            Object::Reference(r) => *r,
            _ => return None,
        };
        return page_from_ref(doc, page_ref);
    }
    // 方式2: /A (Action) 字典，GoTo / GoToR
    if let Ok(action) = node.get(b"A").and_then(|a| a.as_dict()) {
        let s_type = action.get(b"S").ok().and_then(|v| v.as_name().ok())?;
        if s_type == b"GoTo"
            && let Ok(d) = action.get(b"D")
        {
            let page_ref = match d {
                Object::Array(arr) => arr.first()?.as_reference().ok()?,
                Object::Reference(r) => *r,
                _ => return None,
            };
            return page_from_ref(doc, page_ref);
        }
    }
    None
}

fn page_from_ref(doc: &Document, page_ref: ObjectId) -> Option<u32> {
    for (num, &pid) in doc.get_pages().iter() {
        if pid == page_ref {
            return Some(*num);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────
// sanitize：去除编辑权限 / 展平表单 / 去除交互动作 / 删除签名
// ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn sanitize(
    input: std::path::PathBuf,
    output: std::path::PathBuf,
    remove_perms: bool,
    flatten_forms: bool,
    remove_forms: bool,
    remove_actions: bool,
    remove_sigs: bool,
    remove_annots: bool,
    clean_meta: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    // 默认全部执行
    let all = !remove_perms
        && !flatten_forms
        && !remove_forms
        && !remove_actions
        && !remove_sigs
        && !remove_annots
        && !clean_meta;
    let do_perms = all || remove_perms;
    let do_flatten = all || flatten_forms;
    let do_remove_forms = all || remove_forms;
    let do_actions = all || remove_actions;
    let do_sigs = all || remove_sigs;
    let do_annots = all || remove_annots;
    let do_meta = all || clean_meta;

    let mut doc = crate::util::load_document(&input, password)?;
    let mut report: Vec<String> = Vec::new();

    // ── 1. 去除加密权限 ────────────────────────────────────────
    if do_perms {
        let removed = sanitize_permissions(&mut doc);
        report.push(format!(
            "  权限清理: {}",
            if removed {
                "✅ 已移除 Encrypt / Perms"
            } else {
                "— 无加密字典"
            }
        ));
    }

    // ── 2. 删除所有注释 ───────────────────────────────────────
    if do_annots {
        let mut count = 0usize;
        for pid in doc.get_pages().values().cloned().collect::<Vec<_>>() {
            if let Ok(d) = doc.get_object_mut(pid).and_then(|o| o.as_dict_mut())
                && d.remove(b"Annots").is_some()
            {
                count += 1;
            }
        }
        report.push(format!("  注释删除: ✅ 清除 {} 页的 Annots", count));
    }

    // ── 3. 展平表单（优先）或直接删除表单 ──────────────────────
    if do_flatten {
        let n = sanitize_flatten_forms(&mut doc)?;
        report.push(format!("  表单展平: ✅ 处理 {} 个字段，AcroForm 已移除", n));
    } else if do_remove_forms
        && let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
        && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
    {
        if root.remove(b"AcroForm").is_some() {
            report.push("  表单删除: ✅ AcroForm 已直接删除".to_string());
        } else {
            report.push("  表单删除: — 无 AcroForm".to_string());
        }
    }

    // ── 4. 去除页面 / 目录交互动作 ────────────────────────────
    if do_actions {
        let n = sanitize_remove_actions(&mut doc)?;
        report.push(format!("  交互动作: ✅ 清理 {} 处动作/触发器", n));
    }

    // ── 5. 删除数字签名 ───────────────────────────────────────
    if do_sigs {
        let n = sanitize_remove_signatures(&mut doc)?;
        report.push(format!("  数字签名: ✅ 移除 {} 个签名字段/注释", n));
    }

    // ── 6. 清理元数据 ─────────────────────────────────────────
    if do_meta {
        doc.trailer.remove(b"Info");
        if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
            && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
        {
            root.remove(b"Metadata");
            root.remove(b"Permissions");
        }
        report.push("  元数据:   ✅ Info / Metadata / Permissions 已清除".to_string());
    }

    crate::util::save_document(&mut doc, output)?;
    println!("✨ sanitize 完成：");
    for line in &report {
        println!("{}", line);
    }
    Ok(())
}

// ── 实现：去除加密权限 ────────────────────────────────────────

fn sanitize_permissions(doc: &mut Document) -> bool {
    if doc.trailer.get(b"Encrypt").is_err() {
        return false;
    }
    // 移除 Encrypt 间接对象
    if let Ok(enc_id) = doc.trailer.get(b"Encrypt").and_then(|o| o.as_reference()) {
        doc.objects.remove(&enc_id);
    }
    doc.trailer.remove(b"Encrypt");
    // 同时清理 Catalog 中 Permissions / Perms
    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
        && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
    {
        root.remove(b"Perms");
        root.remove(b"Permissions");
    }
    true
}

// ── 实现：展平 AcroForm 表单 ─────────────────────────────────
//
// 策略：遍历 AcroForm/Fields 数组中每个字段，取其 /AP /N（正常外观）
// 流，把流内容追加到对应页面的内容流末尾，然后删除 AcroForm。
// 对于没有 AP 的字段，仅删除注释引用（使其不可交互）。

fn sanitize_flatten_forms(doc: &mut Document) -> anyhow::Result<usize> {
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;

    // 提前收集所有字段 ID
    let field_ids: Vec<ObjectId> = {
        let root = doc.get_object(root_id)?.as_dict()?;
        let acro = match root.get(b"AcroForm") {
            Ok(o) => o.clone(),
            Err(_) => return Ok(0),
        };
        let acro_dict = if let Ok(id) = acro.as_reference() {
            doc.get_object(id)?.as_dict()?.clone()
        } else {
            acro.as_dict()?.clone()
        };
        collect_field_ids(doc, &acro_dict)?
    };

    let mut count = 0usize;

    // 建立 annotation → page_id 映射（用于定位字段所在页）
    let page_annot_map = build_page_annot_map(doc)?;

    for field_id in &field_ids {
        // 取 AP/N 外观流
        let ap_content: Option<Vec<u8>> = {
            let field_obj = doc.get_object(*field_id);
            if let Ok(field_dict) = field_obj.and_then(|o| o.as_dict()) {
                if let Ok(ap) = field_dict.get(b"AP").and_then(|o| o.as_dict()) {
                    if let Ok(n_ref) = ap.get(b"N").and_then(|o| o.as_reference()) {
                        if let Ok(stream) = doc.get_object(n_ref).and_then(|o| o.as_stream()) {
                            stream.decompressed_content().ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        // 找到该字段所在页面
        if let Some(ap_bytes) = ap_content
            && let Some(&page_id) = page_annot_map.get(field_id)
        {
            // 在页面内容流末尾追加外观内容（用 q/Q 包裹隔离图形状态）
            let mut existing = doc.get_page_content(page_id).unwrap_or_default();
            existing.extend_from_slice(b"\nq\n");
            existing.extend_from_slice(&ap_bytes);
            existing.extend_from_slice(b"\nQ\n");
            doc.change_page_content(page_id, existing)?;
            count += 1;
        }

        // 从页面 Annots 数组中删除该字段引用
        if let Some(&page_id) = page_annot_map.get(field_id) {
            remove_annot_ref(doc, page_id, *field_id);
        }
    }

    // 删除 AcroForm
    if let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut()) {
        root.remove(b"AcroForm");
    }

    Ok(count)
}

fn collect_field_ids(
    doc: &Document,
    acro_dict: &lopdf::Dictionary,
) -> anyhow::Result<Vec<ObjectId>> {
    let mut ids = Vec::new();
    if let Ok(fields_obj) = acro_dict.get(b"Fields") {
        let arr = match fields_obj {
            Object::Array(a) => a.clone(),
            Object::Reference(r) => doc.get_object(*r)?.as_array()?.clone(),
            _ => return Ok(ids),
        };
        for item in &arr {
            if let Ok(id) = item.as_reference() {
                ids.push(id);
                // 递归收集子字段
                if let Ok(child) = doc.get_object(id).and_then(|o| o.as_dict())
                    && child.get(b"Kids").is_ok()
                {
                    let sub = collect_field_ids(doc, child)?;
                    ids.extend(sub);
                }
            }
        }
    }
    Ok(ids)
}

/// page_id → annotation ObjectId 的反向映射
fn build_page_annot_map(
    doc: &Document,
) -> anyhow::Result<std::collections::HashMap<ObjectId, ObjectId>> {
    let mut map = std::collections::HashMap::new();
    for &page_id in doc.get_pages().values() {
        if let Ok(page_dict) = doc.get_object(page_id).and_then(|o| o.as_dict())
            && let Ok(annots) = page_dict.get(b"Annots").and_then(|o| o.as_array())
        {
            for ann in annots {
                if let Ok(ann_id) = ann.as_reference() {
                    map.insert(ann_id, page_id);
                }
            }
        }
    }
    Ok(map)
}

fn remove_annot_ref(doc: &mut Document, page_id: ObjectId, target: ObjectId) {
    if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut())
        && let Ok(annots) = page_dict.get_mut(b"Annots").and_then(|o| o.as_array_mut())
    {
        annots.retain(|o| o.as_reference().ok() != Some(target));
    }
}

// ── 实现：去除交互动作 ────────────────────────────────────────

fn sanitize_remove_actions(doc: &mut Document) -> anyhow::Result<usize> {
    let mut count = 0usize;

    // Catalog 级：AA / OpenAction / URI
    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
        && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
    {
        for key in &[b"AA".as_ref(), b"OpenAction", b"URI"] {
            if root.remove(key).is_some() {
                count += 1;
            }
        }
    }

    // 页面级：AA / OpenAction；注释级：A / AA（保留无动作注释）
    let page_ids: Vec<ObjectId> = doc.get_pages().values().cloned().collect();
    for page_id in page_ids {
        // 页面字典自身的 AA
        {
            if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                for key in &[b"AA".as_ref(), b"OpenAction"] {
                    if page_dict.remove(key).is_some() {
                        count += 1;
                    }
                }
            }
        }
        // 收集注释 ID
        let annot_ids: Vec<ObjectId> = {
            if let Ok(page_dict) = doc.get_object(page_id).and_then(|o| o.as_dict()) {
                if let Ok(annots) = page_dict.get(b"Annots").and_then(|o| o.as_array()) {
                    annots
                        .iter()
                        .filter_map(|o| o.as_reference().ok())
                        .collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };
        for ann_id in annot_ids {
            if let Ok(ann) = doc.get_object_mut(ann_id).and_then(|o| o.as_dict_mut()) {
                for key in &[b"A".as_ref(), b"AA"] {
                    if ann.remove(key).is_some() {
                        count += 1;
                    }
                }
            }
        }
    }
    Ok(count)
}

// ── 实现：删除数字签名 ────────────────────────────────────────

fn sanitize_remove_signatures(doc: &mut Document) -> anyhow::Result<usize> {
    let mut count = 0usize;
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;

    // 收集 AcroForm 中 /Sig 类型的字段 ID
    let sig_field_ids: Vec<ObjectId> = {
        let root = doc.get_object(root_id)?.as_dict()?;
        let acro_obj = match root.get(b"AcroForm") {
            Ok(o) => o.clone(),
            Err(_) => return Ok(0),
        };
        let acro_dict = if let Ok(id) = acro_obj.as_reference() {
            doc.get_object(id)?.as_dict()?.clone()
        } else {
            acro_obj.as_dict()?.clone()
        };
        let all_fields = collect_field_ids(doc, &acro_dict)?;
        let mut sigs = Vec::new();
        for id in all_fields {
            let is_sig = {
                if let Ok(d) = doc.get_object(id).and_then(|o| o.as_dict()) {
                    d.get(b"FT").and_then(|v| v.as_name()).ok() == Some(b"Sig")
                } else {
                    false
                }
            };
            if is_sig {
                sigs.push(id);
            }
        }
        sigs
    };

    let page_annot_map = build_page_annot_map(doc)?;

    for sig_id in &sig_field_ids {
        // 删除关联的 /V (Sig 值流对象)
        let v_id: Option<ObjectId> = doc
            .get_object(*sig_id)
            .and_then(|o| o.as_dict())
            .ok()
            .and_then(|d| d.get(b"V").and_then(|o| o.as_reference()).ok());
        if let Some(vid) = v_id {
            doc.objects.remove(&vid);
        }
        // 从页面 Annots 移除引用
        if let Some(&page_id) = page_annot_map.get(sig_id) {
            remove_annot_ref(doc, page_id, *sig_id);
        }
        // 删除字段对象
        doc.objects.remove(sig_id);
        count += 1;
    }

    // 从 AcroForm Fields 数组中移除这些 ID
    if !sig_field_ids.is_empty() {
        let acro_obj_id: Option<ObjectId> = doc
            .get_object(root_id)
            .and_then(|o| o.as_dict())
            .ok()
            .and_then(|d| d.get(b"AcroForm").and_then(|o| o.as_reference()).ok());
        if let Some(acro_id) = acro_obj_id {
            if let Ok(acro) = doc.get_object_mut(acro_id).and_then(|o| o.as_dict_mut()) {
                acro.remove(b"SigFlags");
                if let Ok(fields) = acro.get_mut(b"Fields").and_then(|o| o.as_array_mut()) {
                    fields.retain(|o| {
                        o.as_reference()
                            .map(|id| !sig_field_ids.contains(&id))
                            .unwrap_or(true)
                    });
                }
            }
        } else if let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut()) {
            // AcroForm 是内联字典
            if let Ok(acro) = root.get_mut(b"AcroForm").and_then(|o| o.as_dict_mut()) {
                acro.remove(b"SigFlags");
                if let Ok(fields) = acro.get_mut(b"Fields").and_then(|o| o.as_array_mut()) {
                    fields.retain(|o| {
                        o.as_reference()
                            .map(|id| !sig_field_ids.contains(&id))
                            .unwrap_or(true)
                    });
                }
            }
        }
    }

    Ok(count)
}
